//! Orchestrator helper functions — self-contained utilities extracted from mod.rs.

use std::path::Path;

use tracing::{error, info, warn};

use crate::config::{CliOverrides, LabelOverrides, Route, RunnerConfig};
use crate::error::{Error, Result};
use crate::executor::{AgentAdapter, ClaudeAdapter, StageResult};
use crate::git::GitClient;
use crate::github;
use crate::github::GitHubClient;
use crate::planner;
use crate::state::{RunRecord, StageStatus};
use crate::workflow;
use crate::workflow::StageKind;

/// Subject-specific fields passed into `execute_stages` to parameterise the two
/// pipelines (issue vs PR) without duplicating the loop body.
pub(super) struct StageContext<'a> {
    pub subject_number: u64,
    pub subject_label: &'static str, // "issue" or "pr" — used as a log field value
    pub repo: &'a str,
    pub branch: &'a str,
    pub run_id: &'a str,
    /// Present only for issue pipelines — required by `handle_open_pr`.
    pub issue: Option<&'a github::IssueCandidate>,
    /// Whether to create an early draft PR after the plan stage and promote it on open_pr.
    pub draft_pr: bool,
    pub cli_overrides: &'a CliOverrides,
    pub label_overrides: &'a LabelOverrides,
}

/// Execute the ordered stage list for one run, returning `true` when all
/// non-optional stages succeeded.
///
/// Both `process_issue_with_overrides` and `process_pr_with_overrides` delegate
/// their inner stage-execution loop here. The `ctx` argument captures the two
/// surface differences between the pipelines: the tracing label and the optional
/// `IssueCandidate` needed by `handle_open_pr`.
#[allow(clippy::too_many_arguments)]
pub(super) async fn execute_stages(
    stages: &[planner::PlannedStage],
    ctx: &StageContext<'_>,
    config: &RunnerConfig,
    route: &Route,
    record: &mut RunRecord,
    worktree_dir: &Path,
    gh: &dyn GitHubClient,
    git: &dyn GitClient,
    validation: &[String],
) -> Result<bool> {
    let number = ctx.subject_number;
    let subject = ctx.subject_label;

    let mut all_succeeded = true;
    let mut pending_breadcrumb: Option<String> = None;

    for (stage_idx, planned_stage) in stages.iter().enumerate() {
        // Clone the stage and prepend breadcrumb from the previous stage when available.
        let stage_for_exec = if let Some(ref crumb) = pending_breadcrumb {
            let mut s = planned_stage.clone();
            s.prompt = format!(
                "## Context from previous stage\n\n{crumb}\n\n---\n\n{}",
                s.prompt
            );
            s
        } else {
            planned_stage.clone()
        };
        pending_breadcrumb = None;

        // Skip optional stages if condition says so — no hooks run for skipped stages.
        if planned_stage.optional
            && let Some(ref condition) = planned_stage.condition
            && let Ok(o) = tokio::process::Command::new("sh")
                .args(["-c", condition])
                .current_dir(worktree_dir)
                .output()
                .await
            && o.status.success()
        {
            info!(
                subject,
                number,
                stage = planned_stage.kind_name(),
                "skipping optional stage (condition met)"
            );
            continue;
        }

        // Look up per-stage hooks.
        let stage_hooks = config.stage_hooks.get(planned_stage.kind_name());

        // Run pre hooks before stage execution.
        if let Some(h) = stage_hooks
            && !h.pre.is_empty()
        {
            run_stage_hooks(&h.pre, worktree_dir, "pre").await;
        }

        if planned_stage.kind == StageKind::Merge && !config.global.auto_merge {
            info!(
                subject,
                number, "skipping merge stage: auto_merge is disabled"
            );
            continue;
        }

        if planned_stage.kind == StageKind::Merge && !config.security.allows_merge() {
            return Err(Error::Authorization(format!(
                "authorization_level '{}' does not permit merge",
                config.security.authorization_level
            )));
        }

        if planned_stage.kind == StageKind::OpenPr && !config.security.allows_push() {
            return Err(Error::Authorization(format!(
                "authorization_level '{}' does not permit push/PR creation",
                config.security.authorization_level
            )));
        }

        if planned_stage.kind == StageKind::OpenPr {
            let issue = ctx
                .issue
                .expect("OpenPr stage requires IssueCandidate in StageContext");
            let open_pr_success = match handle_open_pr(
                ctx.repo,
                ctx.branch,
                issue,
                record,
                ctx.run_id,
                worktree_dir,
                gh,
                git,
                ctx.draft_pr,
            )
            .await
            {
                Ok(pr) => {
                    record.pr_number = Some(pr.number);
                    info!(
                        subject,
                        number,
                        pr = pr.number,
                        url = pr.html_url,
                        "created PR"
                    );
                    record.record_stage(
                        planned_stage.kind,
                        StageStatus::Succeeded,
                        StageResult {
                            stage: "open_pr".into(),
                            success: true,
                            duration_secs: 0.0,
                            cost_usd: None,
                            output: pr.html_url,
                            files_modified: None,
                        },
                    );
                    true
                }
                Err(e) => {
                    error!(subject, number, error = %e, "failed to create PR");
                    record.record_stage(
                        planned_stage.kind,
                        StageStatus::Failed,
                        StageResult {
                            stage: "open_pr".into(),
                            success: false,
                            duration_secs: 0.0,
                            cost_usd: None,
                            output: e.to_string(),
                            files_modified: None,
                        },
                    );
                    false
                }
            };
            if open_pr_success
                && let Some(h) = stage_hooks
                && !h.post.is_empty()
            {
                run_stage_hooks(&h.post, worktree_dir, "post").await;
            }
            if let Some(h) = stage_hooks
                && !h.finally.is_empty()
            {
                run_stage_hooks(&h.finally, worktree_dir, "finally").await;
            }
            if !open_pr_success {
                all_succeeded = false;
                if !planned_stage.optional {
                    break;
                }
            }
            continue;
        }

        info!(
            subject,
            number,
            stage = planned_stage.kind_name(),
            agentless = planned_stage.agentless,
            "running stage"
        );

        // Agentless stages run a shell command directly — no agent invocation.
        if planned_stage.agentless {
            let command = planned_stage
                .command
                .as_deref()
                .unwrap_or("echo 'no command specified'");
            let start = std::time::Instant::now();
            let output = tokio::process::Command::new("sh")
                .args(["-c", command])
                .current_dir(worktree_dir)
                .output()
                .await;
            let duration = start.elapsed();
            let (success, output_text) = match output {
                Ok(o) => {
                    let stdout = String::from_utf8_lossy(&o.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                    let text = if stderr.is_empty() { stdout } else { stderr };
                    (o.status.success(), text)
                }
                Err(e) => (false, e.to_string()),
            };
            let result = StageResult {
                stage: planned_stage.kind_name().to_string(),
                success,
                duration_secs: duration.as_secs_f64(),
                cost_usd: None,
                output: output_text,
                files_modified: None,
            };
            info!(
                subject,
                number,
                stage = planned_stage.kind_name(),
                success,
                duration = format!("{:.1}s", duration.as_secs_f64()),
                "agentless stage complete"
            );
            let failed = !success;
            record.record_stage(
                planned_stage.kind,
                if success {
                    StageStatus::Succeeded
                } else {
                    StageStatus::Failed
                },
                result,
            );
            if success
                && let Some(h) = stage_hooks
                && !h.post.is_empty()
            {
                run_stage_hooks(&h.post, worktree_dir, "post").await;
            }
            if let Some(h) = stage_hooks
                && !h.finally.is_empty()
            {
                run_stage_hooks(&h.finally, worktree_dir, "finally").await;
            }
            if failed {
                all_succeeded = false;
                if !planned_stage.optional {
                    break;
                }
            }
            continue;
        }

        let stage_model = ctx
            .cli_overrides
            .model
            .as_deref()
            .or(ctx.label_overrides.model.as_deref())
            .or(planned_stage.model.as_deref())
            .or_else(|| config.effective_model(route));
        let stage_skills = if !ctx.cli_overrides.skills.is_empty() {
            ctx.cli_overrides.skills.as_slice()
        } else if !ctx.label_overrides.skills.is_empty() {
            ctx.label_overrides.skills.as_slice()
        } else {
            config.effective_skills(route, planned_stage.skills.as_deref())
        };
        let stage_mcp = config.effective_mcp_config(route, planned_stage.mcp_config.as_deref());
        let stage_syspr = config.effective_append_system_prompt();
        let mut stage_adapter = ClaudeAdapter::new();
        if let Some(m) = stage_model {
            stage_adapter = stage_adapter.model(m);
        }
        if !stage_skills.is_empty() {
            stage_adapter = stage_adapter.skills(stage_skills.iter().cloned());
        }
        if let Some(p) = stage_mcp {
            stage_adapter = stage_adapter.mcp_config(p);
        }
        if let Some(s) = stage_syspr {
            stage_adapter = stage_adapter.append_system_prompt(s);
        }

        match stage_adapter
            .execute_stage(&stage_for_exec, worktree_dir)
            .await
        {
            Ok(result) => {
                let status = if result.success {
                    StageStatus::Succeeded
                } else {
                    StageStatus::Failed
                };
                info!(
                    subject,
                    number,
                    stage = planned_stage.kind_name(),
                    success = result.success,
                    duration = format!("{:.1}s", result.duration_secs),
                    cost = result.cost_usd,
                    "stage complete"
                );
                let failed = !result.success;
                // Load breadcrumb written by this stage for the next stage's context.
                if !failed && stage_idx + 1 < stages.len() {
                    let crumb_path = worktree_dir
                        .join(".forza")
                        .join("breadcrumbs")
                        .join(ctx.run_id)
                        .join(format!("{}.md", planned_stage.kind_name()));
                    if let Ok(content) = std::fs::read_to_string(&crumb_path) {
                        info!(
                            subject,
                            number,
                            stage = planned_stage.kind_name(),
                            "loaded breadcrumb for next stage"
                        );
                        pending_breadcrumb = Some(content);
                    }
                }
                record.record_stage(planned_stage.kind, status, result);
                // After a successful plan stage, create an early draft PR when draft_pr is enabled.
                if !failed
                    && planned_stage.kind == StageKind::Plan
                    && ctx.draft_pr
                    && let Some(issue) = ctx.issue
                {
                    match create_early_draft_pr(
                        ctx.repo,
                        ctx.branch,
                        issue,
                        ctx.run_id,
                        worktree_dir,
                        gh,
                        git,
                    )
                    .await
                    {
                        Ok(pr) => {
                            record.pr_number = Some(pr.number);
                            info!(
                                subject,
                                number,
                                pr = pr.number,
                                "created early draft PR after plan stage"
                            );
                        }
                        Err(e) => {
                            warn!(subject, number, error = %e, "failed to create early draft PR (non-fatal)");
                        }
                    }
                }
                if !failed
                    && let Some(h) = stage_hooks
                    && !h.post.is_empty()
                {
                    run_stage_hooks(&h.post, worktree_dir, "post").await;
                }
                if let Some(h) = stage_hooks
                    && !h.finally.is_empty()
                {
                    run_stage_hooks(&h.finally, worktree_dir, "finally").await;
                }
                if failed {
                    all_succeeded = false;
                    if !planned_stage.optional {
                        break;
                    }
                }
            }
            Err(e) => {
                error!(subject, number, stage = planned_stage.kind_name(), error = %e, "stage error");
                record.record_stage(
                    planned_stage.kind,
                    StageStatus::Failed,
                    StageResult {
                        stage: planned_stage.kind_name().to_string(),
                        success: false,
                        duration_secs: 0.0,
                        cost_usd: None,
                        output: e.to_string(),
                        files_modified: None,
                    },
                );
                if let Some(h) = stage_hooks
                    && !h.finally.is_empty()
                {
                    run_stage_hooks(&h.finally, worktree_dir, "finally").await;
                }
                all_succeeded = false;
                if !planned_stage.optional {
                    break;
                }
            }
        }

        // Check accumulated cost against cap.
        if let Some(cap) = config.global.max_cost_per_issue
            && let Some(total) = record.total_cost_usd
            && total > cap
        {
            warn!(
                subject,
                number,
                total_cost = total,
                cap,
                "cost cap exceeded, aborting remaining stages"
            );
            all_succeeded = false;
            break;
        }

        // Run validation.
        if !validation.is_empty() {
            run_validation(validation, worktree_dir).await;
        }
    }

    Ok(all_succeeded)
}

/// Build a `PlannedStage` for a PR stage in reactive mode.
pub(super) fn build_pr_planned_stage(
    stage: &workflow::Stage,
    pr: &github::PrCandidate,
    run_id: &str,
    all_stages: &[workflow::Stage],
) -> planner::PlannedStage {
    let is_last = all_stages
        .last()
        .map(|s| s.kind == stage.kind)
        .unwrap_or(false);
    let prompt = generate_reactive_pr_prompt(stage.kind, pr, run_id, !is_last);
    planner::PlannedStage {
        kind: stage.kind,
        prompt,
        allowed_files: None,
        validation: vec![],
        optional: stage.optional,
        max_retries: stage.max_retries,
        condition: stage.condition.clone(),
        agentless: stage.agentless,
        command: stage.command.clone(),
        model: stage.model.clone(),
        skills: stage.skills.clone(),
        mcp_config: stage.mcp_config.clone(),
    }
}

/// Generate a stage prompt for a reactive PR maintenance stage.
pub(super) fn generate_reactive_pr_prompt(
    kind: StageKind,
    pr: &github::PrCandidate,
    run_id: &str,
    has_successor: bool,
) -> String {
    let breadcrumb = if has_successor {
        format!(
            "\n\n## Breadcrumb\n\nWhen done, write a brief context summary to \
             `.forza/breadcrumbs/{run_id}/{stage_name}.md`. Include key decisions and \
             any information the next stage will need.",
            stage_name = kind.name()
        )
    } else {
        String::new()
    };

    match kind {
        StageKind::FixCi => format!(
            "Fix the CI failures for PR #{number}: {title}\n\n\
             ## Steps\n\n\
             1. Read the CI failure output (`gh pr checks {number}`).\n\
             2. Identify the failing checks and their error messages.\n\
             3. Fix the failures — compilation errors, test failures, lint issues.\n\
             4. Commit the fixes and push (`git push`).\n\n\
             Branch: `{branch}`{breadcrumb}",
            number = pr.number,
            title = pr.title,
            branch = pr.head_branch,
            breadcrumb = breadcrumb,
        ),
        StageKind::RevisePr => format!(
            "Revise PR #{number}: {title}\n\n\
             ## Steps\n\n\
             1. Check for merge conflicts: `git fetch origin && git rebase origin/{base_branch}`\n\
             2. If the rebase has conflicts, resolve them. Read the conflicting files, \
                understand both sides, and produce the correct merged result.\n\
             3. Check for review feedback: `gh pr view {number} --json reviews`\n\
             4. Address any CHANGES_REQUESTED comments.\n\
             5. Commit any changes and push: `git push --force-with-lease origin {branch}`\n\n\
             Branch: `{branch}` -> `{base_branch}`{breadcrumb}",
            number = pr.number,
            title = pr.title,
            branch = pr.head_branch,
            base_branch = pr.base_branch,
            breadcrumb = breadcrumb,
        ),
        _ => format!(
            "Handle {} stage for PR #{}: {}",
            kind.name(),
            pr.number,
            pr.title,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_open_pr(
    repo: &str,
    branch: &str,
    issue: &github::IssueCandidate,
    record: &RunRecord,
    run_id: &str,
    work_dir: &Path,
    gh: &dyn GitHubClient,
    git: &dyn GitClient,
    draft: bool,
) -> Result<github::PullRequest> {
    let issue_number = issue.number;

    // Commit any uncommitted changes (tracked files only — skip breadcrumbs/temp files).
    let has_changes = git.has_changes(work_dir).await.unwrap_or(false);

    if has_changes {
        let _ = git.stage_tracked(work_dir).await;
        let _ = git
            .commit(
                work_dir,
                &format!("automation: implement changes for #{issue_number}"),
            )
            .await;
    }

    // Rebase on latest origin/main to avoid conflicts with recently merged PRs.
    let _ = git.fetch(work_dir).await;
    let rebase_ok = git.rebase(work_dir, "origin/main").await.unwrap_or(false);
    if !rebase_ok {
        // Rebase failed — abort and try pushing anyway.
        let _ = git.rebase_abort(work_dir).await;
        tracing::warn!(
            issue = issue_number,
            "rebase on origin/main failed, pushing as-is"
        );
    }

    // Push (force to handle stale remote branches from previous failed runs).
    git.push_force(work_dir, branch).await?;

    // Read plan and review breadcrumbs — missing files are silently ignored.
    let crumb_base = work_dir.join(".forza").join("breadcrumbs").join(run_id);
    let plan_crumb = std::fs::read_to_string(crumb_base.join("plan.md")).unwrap_or_default();
    let review_crumb = std::fs::read_to_string(crumb_base.join("review.md")).unwrap_or_default();

    // Get diff stat relative to origin/main — failure yields empty string.
    let diff_stat = git
        .diff_stat(work_dir, "origin/main")
        .await
        .unwrap_or_default();

    let body = build_pr_body(issue, record, &plan_crumb, &review_crumb, &diff_stat);

    // If draft mode and a draft PR was already created after the plan stage,
    // update its body and promote it to ready-for-review instead of creating a new PR.
    if draft && let Some(pr_number) = record.pr_number {
        gh.update_pr_body(repo, pr_number, &body).await?;
        gh.mark_pr_ready_for_review(repo, pr_number).await?;
        // Fetch the PR to return accurate state.
        let existing = gh.fetch_pr_by_branch(repo, branch).await;
        return Ok(match existing {
            Ok(Some(pr)) => github::PullRequest {
                number: pr.number,
                head_branch: pr.head_branch,
                state: pr.state,
                html_url: pr.html_url,
            },
            _ => github::PullRequest {
                number: pr_number,
                head_branch: branch.to_string(),
                state: "open".to_string(),
                html_url: String::new(),
            },
        });
    }

    match gh
        .create_pull_request(repo, branch, &issue.title, &body, work_dir, false)
        .await
    {
        Ok(pr) => Ok(pr),
        Err(e) => {
            let err_msg = e.to_string();
            // If a PR already exists for this branch, find and return it.
            if err_msg.contains("already exists") {
                tracing::info!(
                    issue = issue_number,
                    branch = branch,
                    "PR already exists for branch, looking up existing PR"
                );
                // Extract PR URL from the error message if present.
                if let Some(url) = err_msg.lines().find(|l| l.contains("/pull/")) {
                    let pr_number = url
                        .trim()
                        .rsplit('/')
                        .next()
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0);
                    return Ok(github::PullRequest {
                        number: pr_number,
                        head_branch: branch.to_string(),
                        state: "open".to_string(),
                        html_url: url.trim().to_string(),
                    });
                }
                // Fallback: fetch the PR by branch.
                match gh.fetch_pr_by_branch(repo, branch).await {
                    Ok(Some(pr)) => Ok(github::PullRequest {
                        number: pr.number,
                        head_branch: pr.head_branch,
                        state: pr.state,
                        html_url: pr.html_url,
                    }),
                    _ => Err(e),
                }
            } else {
                Err(e)
            }
        }
    }
}

/// Create a draft PR immediately after the plan stage.
///
/// Pushes the branch and opens a draft PR using the plan breadcrumb as the
/// initial body. The PR number is stored in `record.pr_number` by the caller.
pub(super) async fn create_early_draft_pr(
    repo: &str,
    branch: &str,
    issue: &github::IssueCandidate,
    run_id: &str,
    work_dir: &Path,
    gh: &dyn GitHubClient,
    git: &dyn GitClient,
) -> Result<github::PullRequest> {
    // Push the branch so a PR can be created.
    git.push(work_dir, branch).await?;

    // Use the plan breadcrumb as the initial PR body.
    let crumb_path = work_dir
        .join(".forza")
        .join("breadcrumbs")
        .join(run_id)
        .join("plan.md");
    let plan_crumb = std::fs::read_to_string(&crumb_path).unwrap_or_default();

    let body = if plan_crumb.is_empty() {
        format!(
            "Draft PR for [#{issue_num}]({issue_url}) — {issue_title}.\n\nCloses #{issue_num}",
            issue_num = issue.number,
            issue_url = issue.html_url,
            issue_title = issue.title,
        )
    } else {
        format!(
            "Draft PR for [#{issue_num}]({issue_url}) — {issue_title}.\n\n## Plan\n\n{plan_crumb}\n\nCloses #{issue_num}",
            issue_num = issue.number,
            issue_url = issue.html_url,
            issue_title = issue.title,
        )
    };

    gh.create_pull_request(repo, branch, &issue.title, &body, work_dir, true)
        .await
}

pub(super) fn build_pr_body(
    issue: &github::IssueCandidate,
    record: &RunRecord,
    plan_crumb: &str,
    review_crumb: &str,
    diff_stat: &str,
) -> String {
    let issue_number = issue.number;

    // Build stage table rows.
    let mut stage_rows = String::new();
    let mut total_cost = 0.0f64;
    let mut has_cost = false;
    for stage in &record.stages {
        let name = stage.kind_name();
        let status = match stage.status {
            StageStatus::Succeeded => "succeeded",
            StageStatus::Failed => "failed",
            StageStatus::Skipped => "skipped",
            StageStatus::Pending => "pending",
            StageStatus::Running => "running",
            StageStatus::Waiting => "waiting",
        };
        let (duration_str, cost_str) = if let Some(ref result) = stage.result {
            let dur = format!("{:.1}s", result.duration_secs);
            let cost = if let Some(c) = result.cost_usd {
                has_cost = true;
                total_cost += c;
                format!("${c:.4}")
            } else {
                "-".to_string()
            };
            (dur, cost)
        } else {
            ("-".to_string(), "-".to_string())
        };
        stage_rows.push_str(&format!(
            "| {name} | {status} | {duration_str} | {cost_str} |\n"
        ));
    }

    let mut body = format!(
        "## Summary\n\n\
         Automated implementation for [#{issue_number}]({issue_url}) — {issue_title}.\n\n\
         ## Stages\n\n\
         | Stage | Status | Duration | Cost |\n\
         |-------|--------|----------|------|\n\
         {stage_rows}",
        issue_url = issue.html_url,
        issue_title = issue.title,
    );

    if has_cost {
        body.push_str(&format!("\n**Total cost:** ${total_cost:.4}\n"));
    }

    if !diff_stat.trim().is_empty() {
        body.push_str(&format!("\n## Files changed\n\n```\n{diff_stat}```\n"));
    }

    if !plan_crumb.trim().is_empty() {
        body.push_str(&format!("\n## Plan\n\n{plan_crumb}\n"));
    }

    if !review_crumb.trim().is_empty() {
        body.push_str(&format!("\n## Review\n\n{review_crumb}\n"));
    }

    body.push_str(&format!("\nCloses #{issue_number}"));

    body
}

pub(super) async fn run_stage_hooks(hooks: &[String], work_dir: &Path, label: &str) {
    for cmd in hooks {
        let output = tokio::process::Command::new("sh")
            .args(["-c", cmd])
            .current_dir(work_dir)
            .output()
            .await;
        match output {
            Ok(o) if o.status.success() => {
                tracing::debug!(command = cmd, hook = label, "hook passed");
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                warn!(command = cmd, hook = label, stderr = %stderr, "hook failed (non-fatal)");
            }
            Err(e) => {
                warn!(command = cmd, hook = label, error = %e, "hook command failed to run");
            }
        }
    }
}

pub(super) async fn run_validation(commands: &[String], work_dir: &Path) {
    for cmd in commands {
        let output = tokio::process::Command::new("sh")
            .args(["-c", cmd])
            .current_dir(work_dir)
            .output()
            .await;
        match output {
            Ok(o) if o.status.success() => {
                tracing::debug!(command = cmd, "validation passed");
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                warn!(command = cmd, stderr = %stderr, "validation failed");
            }
            Err(e) => {
                warn!(command = cmd, error = %e, "validation command failed to run");
            }
        }
    }
}
