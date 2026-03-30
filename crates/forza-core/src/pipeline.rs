//! Pipeline — the single execution path for all subjects.
//!
//! This module replaces the three separate processing functions
//! (`process_issue_with_overrides`, `process_pr_with_overrides`,
//! `process_reactive_pr`) with one unified function that handles
//! both issues and PRs identically. Differences between subject types
//! are data (which prompt, which env var), not control flow.

use std::path::{Path, PathBuf};

use crate::tools;

use tracing::{error, info, warn};

use crate::error::Result;
use crate::lifecycle::{self, LifecycleLabels};
use crate::route::MatchedWork;
use crate::run::{self, Outcome, Run, RunStatus, StageResult, StageStatus};
use crate::shell;
use crate::stage::{Execution, StageKind, Workflow};
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
    /// Context files prepended to every agent stage prompt (before skills).
    pub context: Vec<String>,
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
    /// Directory containing tool list overrides (parallel to `prompts/`).
    pub tools_dir: Option<PathBuf>,
    /// Agent name used for agent-specific tool list lookup.
    pub agent: String,
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

    // Ensure forza's internal files are git-ignored so agents don't commit them.
    ensure_forza_gitignore(work_dir).await;

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
            Execution::Native => {
                info!(
                    number = work.subject.number,
                    stage = stage_name,
                    "running native stage"
                );
                execute_native_stage(stage.kind, &work.subject, work_dir, gh, git).await
            }
            Execution::Agent => {
                let configured_model = stage.model.as_deref().or(config.model.as_deref());
                let model = agent.resolve_model(configured_model);
                let base_skills = stage.skills.as_ref().unwrap_or(&config.skills);
                let effective_skills: Vec<String> = config
                    .context
                    .iter()
                    .chain(base_skills.iter())
                    .cloned()
                    .collect();
                let mcp = stage.mcp_config.as_deref().or(config.mcp_config.as_deref());
                let allowed_tools =
                    tools::select_tools(stage.kind, &config.agent, config.tools_dir.as_deref());

                info!(
                    number = work.subject.number,
                    stage = stage_name,
                    model = model.unwrap_or("default"),
                    "running agent stage"
                );

                match agent
                    .execute(
                        stage_name,
                        &full_prompt,
                        work_dir,
                        model,
                        &effective_skills,
                        mcp,
                        config.append_system_prompt.as_deref(),
                        &allowed_tools,
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

        // Auto-commit: if an agent stage left uncommitted changes, commit them.
        // Some agents (e.g. Codex) may not commit even when told to.
        if success && matches!(stage.execution, Execution::Agent) {
            auto_commit_if_needed(work_dir, &work.subject).await;
        }

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

        if !success && !stage.optional {
            all_succeeded = false;
            break;
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
        if !all_succeeded {
            let failed = run.failed_stage();
            run.outcome = Some(Outcome::Failed {
                stage: failed
                    .map(|s| s.kind_name().to_string())
                    .unwrap_or_default(),
                error: failed
                    .and_then(|s| s.result.as_ref())
                    .map(|r| r.output.chars().take(200).collect())
                    .unwrap_or_default(),
            });
        } else {
            let open_pr_succeeded = run
                .stages
                .iter()
                .any(|s| s.kind == StageKind::OpenPr && s.status == StageStatus::Succeeded);
            let merge_succeeded = run
                .stages
                .iter()
                .any(|s| s.kind == StageKind::Merge && s.status == StageStatus::Succeeded);
            let comment_succeeded = run
                .stages
                .iter()
                .any(|s| s.kind == StageKind::Comment && s.status == StageStatus::Succeeded);

            if open_pr_succeeded {
                if let Ok(Some(pr)) = gh
                    .fetch_pr_by_branch(&work.subject.repo, &work.subject.branch)
                    .await
                {
                    run.pr_number = Some(pr.number);
                    run.outcome = Some(Outcome::PrCreated { number: pr.number });
                } else {
                    warn!(
                        number = work.subject.number,
                        branch = %work.subject.branch,
                        "open_pr stage succeeded but no PR found on branch — \
                         gh pr create may have failed silently"
                    );
                    run.outcome = Some(Outcome::NothingToDo);
                }
            } else if merge_succeeded {
                if let Ok(Some(pr)) = gh
                    .fetch_pr_by_branch(&work.subject.repo, &work.subject.branch)
                    .await
                {
                    run.pr_number = Some(pr.number);
                    run.outcome = Some(Outcome::PrMerged { number: pr.number });
                } else {
                    warn!(
                        number = work.subject.number,
                        branch = %work.subject.branch,
                        "merge stage succeeded but no PR found on branch"
                    );
                    run.outcome = Some(Outcome::NothingToDo);
                }
            } else if comment_succeeded {
                run.outcome = Some(Outcome::CommentPosted);
            } else {
                run.outcome = Some(Outcome::NothingToDo);
            }
        }
    }

    // 5. Post failure comment (best-effort).
    if run.status == RunStatus::Failed
        && let Some(failed) = run.failed_stage()
    {
        let stage_name = failed.kind_name().to_string();
        let raw_output = failed
            .result
            .as_ref()
            .map(|r| r.output.as_str())
            .unwrap_or("");
        let truncated: String = {
            let all_lines: Vec<&str> = raw_output.lines().collect();
            let start = all_lines.len().saturating_sub(30);
            let was_truncated = start > 0;
            let joined = all_lines[start..].join("\n");
            let char_count = joined.chars().count();
            if char_count <= 1000 {
                if was_truncated {
                    format!("<...truncated...>\n{joined}")
                } else {
                    joined
                }
            } else {
                let tail: String = joined.chars().skip(char_count - 1000).collect();
                format!("<...truncated...>\n{tail}")
            }
        };
        let comment = format!(
            "**forza pipeline failed** — stage: `{stage_name}` — run: `{run_id}`\n\n```\n{truncated}\n```"
        );
        let _ = gh
            .post_comment(&work.subject.repo, work.subject.number, &comment)
            .await;
    }

    // 6. Release lease.
    lifecycle::release(&work.subject, &run, &config.labels, gh).await;

    // 7. Cleanup worktree.
    if let Some(ref wt) = worktree
        && let Err(e) = git.remove_worktree(repo_dir, wt).await
    {
        warn!(error = %e, "failed to clean worktree (non-fatal)");
    }

    // 8. Persist run record.
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

/// Execute a native stage via direct trait method calls.
///
/// Currently supports `DraftPr` — pushes the branch and creates a draft PR
/// using the `GitClient` and `GitHubClient` traits directly, replacing the
/// previous shell-based implementation.
async fn execute_native_stage(
    kind: StageKind,
    subject: &Subject,
    work_dir: &Path,
    gh: &dyn GitHubClient,
    git: &dyn GitClient,
) -> StageResult {
    let start = std::time::Instant::now();
    match kind {
        StageKind::DraftPr => {
            // 1. Push the branch.
            if let Err(e) = git.push(work_dir, &subject.branch, false).await {
                return StageResult {
                    stage: "draft_pr".into(),
                    success: false,
                    duration_secs: start.elapsed().as_secs_f64(),
                    cost_usd: None,
                    output: format!("failed to push branch: {e}"),
                    files_modified: None,
                };
            }

            // 2. Build PR title and body.
            let title = format!("[WIP] {} (#{})", subject.title, subject.number);
            let body = match tokio::fs::read_to_string(work_dir.join(".plan_breadcrumb.md")).await {
                Ok(content) if !content.trim().is_empty() => content,
                _ => format!(
                    "Work in progress for {} (#{})",
                    subject.title, subject.number
                ),
            };

            // 3. Create the draft PR.
            match gh
                .create_pr(
                    &subject.repo,
                    &subject.branch,
                    &title,
                    &body,
                    true,
                    work_dir,
                )
                .await
            {
                Ok(pr_number) => StageResult {
                    stage: "draft_pr".into(),
                    success: true,
                    duration_secs: start.elapsed().as_secs_f64(),
                    cost_usd: None,
                    output: format!("created draft PR #{pr_number}"),
                    files_modified: None,
                },
                Err(e) => StageResult {
                    stage: "draft_pr".into(),
                    success: false,
                    duration_secs: start.elapsed().as_secs_f64(),
                    cost_usd: None,
                    output: format!("failed to create draft PR: {e}"),
                    files_modified: None,
                },
            }
        }
        other => {
            warn!(stage = other.name(), "unsupported native stage kind");
            StageResult {
                stage: other.name().into(),
                success: false,
                duration_secs: start.elapsed().as_secs_f64(),
                cost_usd: None,
                output: format!(
                    "native execution not implemented for stage: {}",
                    other.name()
                ),
                files_modified: None,
            }
        }
    }
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

/// Ensure forza's internal files are listed in the worktree's `.gitignore`.
///
/// Appends entries for breadcrumb files and the `.forza/` directory if they
/// aren't already present. This prevents agents from accidentally committing
/// forza's working files.
async fn ensure_forza_gitignore(work_dir: &Path) {
    let gitignore_path = work_dir.join(".gitignore");
    let entries = [".plan_breadcrumb.md", ".review_breadcrumb.md", ".forza/"];

    let existing = tokio::fs::read_to_string(&gitignore_path)
        .await
        .unwrap_or_default();

    let mut additions = String::new();
    for entry in &entries {
        if !existing.lines().any(|line| line.trim() == *entry) {
            additions.push_str(entry);
            additions.push('\n');
        }
    }

    if additions.is_empty() {
        return;
    }

    // Append to .gitignore (create if missing).
    let new_content = if existing.is_empty() || existing.ends_with('\n') {
        format!("{existing}# forza internal files\n{additions}")
    } else {
        format!("{existing}\n# forza internal files\n{additions}")
    };

    if let Err(e) = tokio::fs::write(&gitignore_path, new_content).await {
        warn!("failed to update .gitignore: {e}");
    }
}

/// Check for uncommitted changes and commit them on behalf of the agent.
///
/// Some agents (notably Codex) make code changes but don't run `git commit`,
/// even when instructed. This catches that case so downstream stages
/// (validation, draft_pr) see the actual changes.
async fn auto_commit_if_needed(work_dir: &Path, subject: &Subject) {
    // Check for any uncommitted changes (staged or unstaged, including untracked).
    let status = tokio::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(work_dir)
        .output()
        .await;

    let has_changes = match &status {
        Ok(output) => output.status.success() && !output.stdout.is_empty(),
        Err(_) => false,
    };

    if !has_changes {
        return;
    }

    info!(
        number = subject.number,
        "agent left uncommitted changes, auto-committing"
    );

    // Stage everything.
    let add = tokio::process::Command::new("git")
        .args(["add", "-A"])
        .current_dir(work_dir)
        .output()
        .await;
    if add.is_err() || !add.unwrap().status.success() {
        warn!(number = subject.number, "auto-commit: git add failed");
        return;
    }

    // Commit.
    let msg = format!(
        "feat: {} closes #{}",
        subject.title.to_lowercase(),
        subject.number
    );
    let commit = tokio::process::Command::new("git")
        .args(["commit", "-m", &msg])
        .current_dir(work_dir)
        .output()
        .await;
    match commit {
        Ok(output) if output.status.success() => {
            info!(number = subject.number, "auto-commit succeeded");
        }
        _ => {
            warn!(number = subject.number, "auto-commit: git commit failed");
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
///
/// If a previous run left the worktree registered but the directory missing,
/// prune stale entries before retrying.
async fn create_worktree(repo_dir: &Path, branch: &str, git: &dyn GitClient) -> Result<PathBuf> {
    let worktree_dir = repo_dir.join(".worktrees").join(branch.replace('/', "-"));
    match git.create_worktree(repo_dir, branch, &worktree_dir).await {
        Ok(()) => Ok(worktree_dir),
        Err(_) if !worktree_dir.exists() => {
            // Directory missing but registered — prune and retry.
            info!("pruning stale worktree registration, retrying");
            let _ = tokio::process::Command::new("git")
                .args(["worktree", "prune"])
                .current_dir(repo_dir)
                .output()
                .await;
            git.create_worktree(repo_dir, branch, &worktree_dir).await?;
            Ok(worktree_dir)
        }
        Err(e) => Err(e),
    }
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
    use crate::error::Result;
    use crate::route::{MatchedWork, Route, Scope, Trigger};
    use crate::run::{Outcome, StageResult};
    use crate::stage::Stage;
    use crate::subject::{Subject, SubjectKind};
    use crate::traits::{AgentExecutor, GitClient, GitHubClient};
    use async_trait::async_trait;

    struct MockGitHub {
        pr_for_branch: Option<u64>,
    }

    #[async_trait]
    impl GitHubClient for MockGitHub {
        async fn fetch_issue(&self, _repo: &str, _number: u64) -> Result<Subject> {
            unimplemented!()
        }
        async fn fetch_issues_with_labels(
            &self,
            _repo: &str,
            _labels: &[String],
            _limit: usize,
        ) -> Result<Vec<Subject>> {
            Ok(vec![])
        }
        async fn fetch_pr(&self, _repo: &str, _number: u64) -> Result<Subject> {
            unimplemented!()
        }
        async fn fetch_all_open_prs(&self, _repo: &str, _limit: usize) -> Result<Vec<Subject>> {
            Ok(vec![])
        }
        async fn fetch_prs_with_labels(
            &self,
            _repo: &str,
            _labels: &[String],
            _limit: usize,
        ) -> Result<Vec<Subject>> {
            Ok(vec![])
        }
        async fn fetch_pr_by_branch(&self, _repo: &str, branch: &str) -> Result<Option<Subject>> {
            Ok(self.pr_for_branch.map(|number| Subject {
                kind: SubjectKind::Pr,
                number,
                repo: "owner/repo".into(),
                title: "Test PR".into(),
                body: String::new(),
                labels: vec![],
                html_url: String::new(),
                author: "bot".into(),
                branch: branch.to_string(),
                comments: Vec::new(),
                mergeable: None,
                checks_passing: None,
                review_decision: None,
                is_draft: None,
                base_branch: None,
            }))
        }
        async fn add_label(&self, _repo: &str, _number: u64, _label: &str) -> Result<()> {
            Ok(())
        }
        async fn remove_label(&self, _repo: &str, _number: u64, _label: &str) -> Result<()> {
            Ok(())
        }
        async fn create_label(
            &self,
            _repo: &str,
            _name: &str,
            _color: &str,
            _description: &str,
        ) -> Result<()> {
            Ok(())
        }
        async fn post_comment(&self, _repo: &str, _number: u64, _body: &str) -> Result<()> {
            Ok(())
        }
        async fn create_issue(&self, _repo: &str, _title: &str, _body: &str) -> Result<u64> {
            Ok(0)
        }
        async fn create_pr(
            &self,
            _repo: &str,
            _branch: &str,
            _title: &str,
            _body: &str,
            _draft: bool,
            _work_dir: &std::path::Path,
        ) -> Result<u64> {
            Ok(0)
        }
        async fn update_pr_body(&self, _repo: &str, _number: u64, _body: &str) -> Result<()> {
            Ok(())
        }
        async fn mark_pr_ready(&self, _repo: &str, _number: u64) -> Result<()> {
            Ok(())
        }
        async fn authenticated_user(&self) -> Result<String> {
            Ok("bot".into())
        }
    }

    struct NoopGit;

    #[async_trait]
    impl GitClient for NoopGit {
        async fn clone_repo(&self, _url: &str, _dest: &std::path::Path) -> Result<()> {
            Ok(())
        }
        async fn fetch(&self, _repo_dir: &std::path::Path) -> Result<()> {
            Ok(())
        }
        async fn create_branch(&self, _repo_dir: &std::path::Path, _branch: &str) -> Result<()> {
            Ok(())
        }
        async fn checkout(&self, _repo_dir: &std::path::Path, _branch: &str) -> Result<()> {
            Ok(())
        }
        async fn push(
            &self,
            _repo_dir: &std::path::Path,
            _branch: &str,
            _force: bool,
        ) -> Result<()> {
            Ok(())
        }
        async fn create_worktree(
            &self,
            _repo_dir: &std::path::Path,
            _branch: &str,
            _worktree_dir: &std::path::Path,
        ) -> Result<()> {
            Ok(())
        }
        async fn remove_worktree(
            &self,
            _repo_dir: &std::path::Path,
            _worktree_dir: &std::path::Path,
        ) -> Result<()> {
            Ok(())
        }
        async fn list_worktrees(&self, _repo_dir: &std::path::Path) -> Result<Vec<String>> {
            Ok(vec![])
        }
    }

    struct SuccessAgent;

    #[async_trait]
    impl AgentExecutor for SuccessAgent {
        async fn execute(
            &self,
            _stage_name: &str,
            _prompt: &str,
            _work_dir: &std::path::Path,
            _model: Option<&str>,
            _skills: &[String],
            _mcp_config: Option<&str>,
            _append_system_prompt: Option<&str>,
            _allowed_tools: &[String],
        ) -> Result<StageResult> {
            Ok(StageResult {
                stage: "test".into(),
                success: true,
                duration_secs: 0.0,
                cost_usd: None,
                output: "done".into(),
                files_modified: None,
            })
        }
    }

    fn make_work(branch: &str) -> MatchedWork {
        MatchedWork {
            subject: Subject {
                kind: SubjectKind::Issue,
                number: 42,
                repo: "owner/repo".into(),
                title: "Test issue".into(),
                body: String::new(),
                labels: vec![],
                html_url: String::new(),
                author: "user".into(),
                branch: branch.into(),
                comments: Vec::new(),
                mergeable: None,
                checks_passing: None,
                review_decision: None,
                is_draft: None,
                base_branch: None,
            },
            route_name: "test-route".into(),
            route: Route {
                subject_type: SubjectKind::Issue,
                trigger: Trigger::Label("bug".into()),
                workflow: "bug".into(),
                scope: Scope::default(),
                concurrency: 1,
                poll_interval: 60,
                max_retries: None,
                model: None,
                skills: None,
                mcp_config: None,
                validation_commands: None,
            },
            workflow_name: "bug".into(),
        }
    }

    fn make_config() -> PipelineConfig {
        PipelineConfig {
            labels: crate::lifecycle::LifecycleLabels::default(),
            model: None,
            context: vec![],
            skills: vec![],
            mcp_config: None,
            validation: vec![],
            append_system_prompt: None,
            stage_hooks: std::collections::HashMap::new(),
            tools_dir: None,
            agent: "claude".into(),
        }
    }

    #[tokio::test]
    async fn outcome_pr_created_when_open_pr_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let work = make_work("automation/42-test");
        let workflow =
            Workflow::new("bug", vec![Stage::agent(StageKind::OpenPr)]).without_worktree();
        let config = make_config();
        let gh = MockGitHub {
            pr_for_branch: Some(99),
        };
        let run = execute(
            &work,
            &workflow,
            &config,
            dir.path(),
            dir.path(),
            &gh,
            &NoopGit,
            &SuccessAgent,
            &["do the open_pr thing".to_string()],
        )
        .await;
        assert_eq!(run.outcome, Some(Outcome::PrCreated { number: 99 }));
        assert_eq!(run.pr_number, Some(99));
    }

    #[tokio::test]
    async fn outcome_nothing_to_do_when_open_pr_succeeds_but_pr_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let work = make_work("automation/42-test");
        let workflow =
            Workflow::new("bug", vec![Stage::agent(StageKind::OpenPr)]).without_worktree();
        let config = make_config();
        let gh = MockGitHub {
            pr_for_branch: None,
        };
        let run = execute(
            &work,
            &workflow,
            &config,
            dir.path(),
            dir.path(),
            &gh,
            &NoopGit,
            &SuccessAgent,
            &["do the open_pr thing".to_string()],
        )
        .await;
        assert_eq!(run.outcome, Some(Outcome::NothingToDo));
        assert!(run.pr_number.is_none());
    }

    #[tokio::test]
    async fn outcome_pr_merged_when_merge_succeeds_without_open_pr() {
        let dir = tempfile::tempdir().unwrap();
        let work = make_work("automation/42-test");
        let workflow =
            Workflow::new("merge-only", vec![Stage::agent(StageKind::Merge)]).without_worktree();
        let config = make_config();
        let gh = MockGitHub {
            pr_for_branch: Some(77),
        };
        let run = execute(
            &work,
            &workflow,
            &config,
            dir.path(),
            dir.path(),
            &gh,
            &NoopGit,
            &SuccessAgent,
            &["merge the PR".to_string()],
        )
        .await;
        assert_eq!(run.outcome, Some(Outcome::PrMerged { number: 77 }));
        assert_eq!(run.pr_number, Some(77));
    }

    #[tokio::test]
    async fn outcome_comment_posted_when_comment_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let work = make_work("automation/42-test");
        let workflow =
            Workflow::new("research", vec![Stage::agent(StageKind::Comment)]).without_worktree();
        let config = make_config();
        let gh = MockGitHub {
            pr_for_branch: None,
        };
        let run = execute(
            &work,
            &workflow,
            &config,
            dir.path(),
            dir.path(),
            &gh,
            &NoopGit,
            &SuccessAgent,
            &["post the comment".to_string()],
        )
        .await;
        assert_eq!(run.outcome, Some(Outcome::CommentPosted));
    }

    #[tokio::test]
    async fn outcome_nothing_to_do_for_non_special_stages() {
        let dir = tempfile::tempdir().unwrap();
        let work = make_work("automation/42-test");
        let workflow =
            Workflow::new("plan-only", vec![Stage::agent(StageKind::Plan)]).without_worktree();
        let config = make_config();
        let gh = MockGitHub {
            pr_for_branch: None,
        };
        let run = execute(
            &work,
            &workflow,
            &config,
            dir.path(),
            dir.path(),
            &gh,
            &NoopGit,
            &SuccessAgent,
            &["make a plan".to_string()],
        )
        .await;
        assert_eq!(run.outcome, Some(Outcome::NothingToDo));
    }

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
            context: vec![],
            skills: vec![],
            mcp_config: None,
            validation: vec![],
            append_system_prompt: None,
            stage_hooks: std::collections::HashMap::new(),
            tools_dir: None,
            agent: "claude".into(),
        };
        assert!(config.stage_hooks.is_empty());
        assert!(config.validation.is_empty());
    }

    #[test]
    fn failure_comment_truncation_keeps_tail() {
        // Build output where failure details are at the end.
        let mut lines: Vec<String> = (0..50).map(|i| format!("line {i}")).collect();
        lines.push("FAILED: actual error message".to_string());
        let raw_output = lines.join("\n");

        // Replicate the truncation logic from pipeline.rs.
        let all_lines: Vec<&str> = raw_output.lines().collect();
        let start = all_lines.len().saturating_sub(30);
        let was_truncated = start > 0;
        let joined = all_lines[start..].join("\n");
        let char_count = joined.chars().count();
        let truncated: String = if char_count <= 1000 {
            if was_truncated {
                format!("<...truncated...>\n{joined}")
            } else {
                joined
            }
        } else {
            let tail: String = joined.chars().skip(char_count - 1000).collect();
            format!("<...truncated...>\n{tail}")
        };

        // The failure message must appear at the end of the truncated output.
        assert!(
            truncated.ends_with("FAILED: actual error message"),
            "failure message was cut off: {truncated}"
        );
        assert!(truncated.contains("<...truncated...>"));
    }

    #[test]
    fn failure_comment_truncation_long_tail_keeps_last_1000_chars() {
        // Build a single-line output longer than 1000 chars so char-level truncation fires.
        let long_suffix = "X".repeat(900);
        let raw_output = format!("{}\nFAILED: {long_suffix}", "padding\n".repeat(40));

        let all_lines: Vec<&str> = raw_output.lines().collect();
        let start = all_lines.len().saturating_sub(30);
        let was_truncated = start > 0;
        let joined = all_lines[start..].join("\n");
        let char_count = joined.chars().count();
        let truncated: String = if char_count <= 1000 {
            if was_truncated {
                format!("<...truncated...>\n{joined}")
            } else {
                joined
            }
        } else {
            let tail: String = joined.chars().skip(char_count - 1000).collect();
            format!("<...truncated...>\n{tail}")
        };

        // Output should end with the tail of the long suffix.
        assert!(truncated.ends_with(&long_suffix));
        assert!(truncated.contains("<...truncated...>"));
    }
}
