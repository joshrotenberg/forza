//! Orchestrator helper functions — self-contained utilities extracted from mod.rs.

use std::path::Path;

use tracing::warn;

use crate::error::Result;
use crate::git::GitClient;
use crate::github;
use crate::github::GitHubClient;
use crate::planner;
use crate::state::{RunRecord, StageStatus};
use crate::workflow;
use crate::workflow::StageKind;

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
