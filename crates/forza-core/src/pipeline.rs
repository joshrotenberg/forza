//! Pipeline — the single execution path for all subjects.
//!
//! This module replaces the three separate processing functions
//! (`process_issue_with_overrides`, `process_pr_with_overrides`,
//! `process_reactive_pr`) with one unified function that handles
//! both issues and PRs identically. Differences between subject types
//! are data (which prompt, which env var), not control flow.

use std::path::{Path, PathBuf};

use tracing::{error, info, warn};

use crate::error::Result;
use crate::lifecycle::{self, LifecycleLabels};
use crate::route::MatchedWork;
use crate::run::{self, Outcome, Run, RunStatus, StageResult, StageStatus};
use crate::shell;
use crate::stage::{Execution, Workflow};
use crate::subject::Subject;
use crate::traits::{AgentExecutor, GitClient, GitHubClient};

/// Configuration for the pipeline execution.
///
/// Holds the resolved settings that affect how stages are executed.
/// Built from the global config + route overrides before calling `execute`.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Lifecycle label names.
    pub labels: LifecycleLabels,
    /// Default model for agent stages.
    pub model: Option<String>,
    /// Default skill files.
    pub skills: Vec<String>,
    /// Default MCP config path.
    pub mcp_config: Option<String>,
    /// Validation commands to run between stages.
    pub validation: Vec<String>,
    /// System prompt to append to all agent stages.
    pub append_system_prompt: Option<String>,
    /// Per-stage hooks.
    pub stage_hooks: std::collections::HashMap<String, StageHooks>,
}

/// Pre/post/finally hooks for a stage kind.
#[derive(Debug, Clone, Default)]
pub struct StageHooks {
    /// Commands to run before the stage. Failure aborts the stage.
    pub pre: Vec<String>,
    /// Commands to run after a successful stage. Failure marks the stage failed.
    pub post: Vec<String>,
    /// Commands to run regardless of outcome (cleanup, notifications).
    pub finally: Vec<String>,
}

/// Execute a matched work item through its workflow pipeline.
///
/// This is the single execution path for all subjects — issues and PRs alike.
/// It replaces three separate functions with one ~100 line function.
///
/// # Flow
///
/// 1. Acquire processing lease (in-progress label)
/// 2. Create worktree (if needed)
/// 3. Execute stages sequentially
/// 4. Release lease (complete/failed label)
/// 5. Cleanup worktree
/// 6. Persist run record
#[allow(clippy::too_many_arguments)]
pub async fn execute(
    work: &MatchedWork,
    workflow: &Workflow,
    config: &PipelineConfig,
    state_dir: &Path,
    repo_dir: &Path,
    gh: &dyn GitHubClient,
    git: &dyn GitClient,
    agent: &dyn AgentExecutor,
    prompts: &[String],
) -> Run {
    let run_id = run::generate_run_id();
    let mut run = Run::new(
        &run_id,
        &work.subject.repo,
        work.subject.number,
        work.subject.kind,
        &work.route_name,
        &work.workflow_name,
        &work.subject.branch,
    );

    // 1. Acquire lease.
    lifecycle::acquire(&work.subject, &config.labels, gh).await;

    // 2. Create worktree (if needed).
    let worktree: Option<PathBuf> = if workflow.needs_worktree {
        match create_worktree(repo_dir, &work.subject.branch, git).await {
            Ok(wt) => {
                info!(
                    number = work.subject.number,
                    worktree = %wt.display(),
                    "created worktree"
                );
                Some(wt)
            }
            Err(e) => {
                error!(number = work.subject.number, error = %e, "failed to create worktree");
                run.finish(RunStatus::Failed);
                run.outcome = Some(Outcome::Failed {
                    stage: "setup".into(),
                    error: e.to_string(),
                });
                lifecycle::release(&work.subject, &run, &config.labels, gh).await;
                return run;
            }
        }
    } else {
        None
    };
    let work_dir = worktree.as_deref().unwrap_or(repo_dir);

    // 3. Execute stages.
    let mut all_succeeded = true;
    let mut pending_breadcrumb: Option<String> = None;

    for (i, (stage, prompt)) in workflow.stages.iter().zip(prompts.iter()).enumerate() {
        let stage_name = stage.kind.name();

        // Prepend breadcrumb from previous stage.
        let full_prompt = if let Some(ref bc) = pending_breadcrumb {
            format!("## Context from previous stage\n\n{bc}\n\n---\n\n{prompt}")
        } else {
            prompt.clone()
        };

        // Evaluate condition.
        if let Some(ref condition) = stage.condition {
            let should_run = shell::check_condition(
                condition,
                work_dir,
                &work.subject,
                &run_id,
                &work.route_name,
                &work.workflow_name,
            )
            .await;
            if !should_run {
                info!(
                    number = work.subject.number,
                    stage = stage_name,
                    "stage condition not met, skipping"
                );
                if stage.optional {
                    run.record_skipped(stage.kind);
                    continue;
                }
                run.record_stage(
                    stage.kind,
                    StageStatus::Failed,
                    StageResult {
                        stage: stage_name.into(),
                        success: false,
                        duration_secs: 0.0,
                        cost_usd: None,
                        output: "condition not met".into(),
                        files_modified: None,
                    },
                );
                all_succeeded = false;
                break;
            }
        }

        // Pre-hooks.
        if let Some(hooks) = config.stage_hooks.get(stage_name)
            && let Err((cmd, result)) = shell::run_all(
                &hooks.pre,
                work_dir,
                &work.subject,
                &run_id,
                &work.route_name,
                &work.workflow_name,
            )
            .await
        {
            warn!(
                number = work.subject.number,
                stage = stage_name,
                command = cmd,
                "pre-hook failed"
            );
            run.record_stage(
                stage.kind,
                StageStatus::Failed,
                StageResult {
                    stage: stage_name.into(),
                    success: false,
                    duration_secs: result.duration.as_secs_f64(),
                    cost_usd: None,
                    output: format!("pre-hook failed: {cmd}"),
                    files_modified: None,
                },
            );
            all_succeeded = false;
            run_finally_hooks(
                config,
                stage_name,
                work_dir,
                &work.subject,
                &run_id,
                &work.route_name,
                &work.workflow_name,
            )
            .await;
            break;
        }

        info!(
            number = work.subject.number,
            stage = stage_name,
            agentless = stage.is_agentless(),
            "running stage"
        );

        // Execute the stage.
        let result = match &stage.execution {
            Execution::Shell { command } => {
                let shell_result = shell::run(
                    command,
                    work_dir,
                    &work.subject,
                    &run_id,
                    &work.route_name,
                    &work.workflow_name,
                )
                .await;
                StageResult {
                    stage: stage_name.into(),
                    success: shell_result.success,
                    duration_secs: shell_result.duration.as_secs_f64(),
                    cost_usd: None,
                    output: shell_result.output,
                    files_modified: None,
                }
            }
            Execution::Agent => {
                let model = stage.model.as_deref().or(config.model.as_deref());
                let skills = stage.skills.as_ref().unwrap_or(&config.skills);
                let mcp = stage.mcp_config.as_deref().or(config.mcp_config.as_deref());

                match agent
                    .execute(
                        &full_prompt,
                        work_dir,
                        model,
                        skills,
                        mcp,
                        config.append_system_prompt.as_deref(),
                    )
                    .await
                {
                    Ok(result) => result,
                    Err(e) => {
                        error!(
                            number = work.subject.number,
                            stage = stage_name,
                            error = %e,
                            "agent execution error"
                        );
                        StageResult {
                            stage: stage_name.into(),
                            success: false,
                            duration_secs: 0.0,
                            cost_usd: None,
                            output: e.to_string(),
                            files_modified: None,
                        }
                    }
                }
            }
        };

        let success = result.success;
        info!(
            number = work.subject.number,
            stage = stage_name,
            success,
            duration = format!("{:.1}s", result.duration_secs),
            "stage complete"
        );

        // Record stage result.
        let stage_status = if success {
            StageStatus::Succeeded
        } else {
            StageStatus::Failed
        };
        run.record_stage(stage.kind, stage_status, result);

        // Post-hooks (only on success).
        if success
            && let Some(hooks) = config.stage_hooks.get(stage_name)
            && let Err((cmd, _)) = shell::run_all(
                &hooks.post,
                work_dir,
                &work.subject,
                &run_id,
                &work.route_name,
                &work.workflow_name,
            )
            .await
        {
            warn!(
                number = work.subject.number,
                stage = stage_name,
                command = cmd,
                "post-hook failed, marking stage failed"
            );
            // Override the last stage status to failed.
            if let Some(last) = run.stages.last_mut() {
                last.status = StageStatus::Failed;
            }
            all_succeeded = false;
            run_finally_hooks(
                config,
                stage_name,
                work_dir,
                &work.subject,
                &run_id,
                &work.route_name,
                &work.workflow_name,
            )
            .await;
            break;
        }

        // Finally hooks (always).
        run_finally_hooks(
            config,
            stage_name,
            work_dir,
            &work.subject,
            &run_id,
            &work.route_name,
            &work.workflow_name,
        )
        .await;

        // Load breadcrumb for next stage.
        pending_breadcrumb = load_breadcrumb(&run_id, stage_name, work_dir).await;
        if pending_breadcrumb.is_some() {
            info!(
                number = work.subject.number,
                stage = stage_name,
                "loaded breadcrumb for next stage"
            );
        }

        // Validation (between stages, not after the last one).
        if success
            && i < workflow.stages.len() - 1
            && !config.validation.is_empty()
            && let Err((cmd, result)) = shell::run_all(
                &config.validation,
                work_dir,
                &work.subject,
                &run_id,
                &work.route_name,
                &work.workflow_name,
            )
            .await
        {
            warn!(
                number = work.subject.number,
                command = cmd,
                "validation failed"
            );
            run.record_stage(
                stage.kind,
                StageStatus::Failed,
                StageResult {
                    stage: format!("validation after {stage_name}"),
                    success: false,
                    duration_secs: result.duration.as_secs_f64(),
                    cost_usd: None,
                    output: format!("validation failed: {cmd}\n{}", result.output),
                    files_modified: None,
                },
            );
            all_succeeded = false;
            break;
        }

        if !success {
            all_succeeded = false;
            if !stage.optional {
                break;
            }
        }
    }

    // 4. Determine outcome.
    let final_status = if all_succeeded {
        RunStatus::Succeeded
    } else {
        RunStatus::Failed
    };
    run.finish(final_status);

    if run.outcome.is_none() {
        run.outcome = Some(if !all_succeeded {
            let failed = run.failed_stage();
            Outcome::Failed {
                stage: failed
                    .map(|s| s.kind_name().to_string())
                    .unwrap_or_default(),
                error: failed
                    .and_then(|s| s.result.as_ref())
                    .map(|r| r.output.chars().take(200).collect())
                    .unwrap_or_default(),
            }
        } else {
            // Default success outcome — callers can override (e.g., PrCreated)
            Outcome::NothingToDo
        });
    }

    // 5. Release lease.
    lifecycle::release(&work.subject, &run, &config.labels, gh).await;

    // 6. Cleanup worktree.
    if let Some(ref wt) = worktree
        && let Err(e) = git.remove_worktree(repo_dir, wt).await
    {
        warn!(error = %e, "failed to clean worktree (non-fatal)");
    }

    // 7. Persist run record.
    if let Err(e) = save_run(&run, state_dir) {
        warn!(error = %e, "failed to save run record (non-fatal)");
    }

    info!(
        number = work.subject.number,
        run_id = run.run_id,
        status = %run.status,
        outcome = ?run.outcome,
        "run complete"
    );

    run
}

/// Run finally hooks for a stage (always runs, regardless of outcome).
async fn run_finally_hooks(
    config: &PipelineConfig,
    stage_name: &str,
    work_dir: &Path,
    subject: &Subject,
    run_id: &str,
    route: &str,
    workflow: &str,
) {
    if let Some(hooks) = config.stage_hooks.get(stage_name) {
        for cmd in &hooks.finally {
            let _ = shell::run(cmd, work_dir, subject, run_id, route, workflow).await;
        }
    }
}

/// Load a breadcrumb file from a completed stage.
async fn load_breadcrumb(run_id: &str, stage_name: &str, work_dir: &Path) -> Option<String> {
    let path = work_dir
        .join(".forza")
        .join("breadcrumbs")
        .join(run_id)
        .join(format!("{stage_name}.md"));
    tokio::fs::read_to_string(&path).await.ok()
}

/// Create a worktree for isolated execution.
async fn create_worktree(repo_dir: &Path, branch: &str, git: &dyn GitClient) -> Result<PathBuf> {
    let worktree_dir = repo_dir.join(".worktrees").join(branch.replace('/', "-"));
    git.create_worktree(repo_dir, branch, &worktree_dir).await?;
    Ok(worktree_dir)
}

/// Save a run record to the state directory.
fn save_run(run: &Run, state_dir: &Path) -> Result<()> {
    let dir = state_dir.join("runs");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", run.run_id));
    let json = serde_json::to_string_pretty(run)?;

    // Atomic write: write to temp file, then rename.
    let tmp_path = path.with_extension("json.tmp");
    std::fs::write(&tmp_path, &json)?;
    std::fs::rename(&tmp_path, &path)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_run_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let run = Run::new(
            "run-test-001",
            "owner/repo",
            42,
            crate::subject::SubjectKind::Issue,
            "bugfix",
            "bug",
            "automation/42-test",
        );
        save_run(&run, dir.path()).unwrap();

        let path = dir.path().join("runs").join("run-test-001.json");
        assert!(path.exists());

        let content = std::fs::read_to_string(&path).unwrap();
        let restored: Run = serde_json::from_str(&content).unwrap();
        assert_eq!(restored.run_id, "run-test-001");
        assert_eq!(restored.subject_number, 42);
    }

    #[test]
    fn save_run_atomic_no_partial_writes() {
        let dir = tempfile::tempdir().unwrap();
        let run = Run::new(
            "run-test-002",
            "owner/repo",
            1,
            crate::subject::SubjectKind::Pr,
            "auto-merge",
            "pr-merge",
            "automation/1-fix",
        );
        save_run(&run, dir.path()).unwrap();

        // No .tmp file should remain.
        let tmp = dir.path().join("runs").join("run-test-002.json.tmp");
        assert!(!tmp.exists());
    }

    #[tokio::test]
    async fn load_breadcrumb_returns_none_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let result = load_breadcrumb("run-1", "plan", dir.path()).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn load_breadcrumb_reads_file() {
        let dir = tempfile::tempdir().unwrap();
        let bc_dir = dir.path().join(".forza").join("breadcrumbs").join("run-1");
        std::fs::create_dir_all(&bc_dir).unwrap();
        std::fs::write(bc_dir.join("plan.md"), "# Plan\nDo the thing.").unwrap();

        let result = load_breadcrumb("run-1", "plan", dir.path()).await;
        assert_eq!(result.as_deref(), Some("# Plan\nDo the thing."));
    }

    #[test]
    fn pipeline_config_default_hooks() {
        let config = PipelineConfig {
            labels: LifecycleLabels::default(),
            model: None,
            skills: vec![],
            mcp_config: None,
            validation: vec![],
            append_system_prompt: None,
            stage_hooks: std::collections::HashMap::new(),
        };
        assert!(config.stage_hooks.is_empty());
        assert!(config.validation.is_empty());
    }
}
