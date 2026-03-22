//! Orchestrator — the main pipeline that processes a single issue end-to-end.

mod helpers;
use helpers::*;

use std::collections::{HashMap, VecDeque};

use indexmap::IndexMap;
use std::path::{Path, PathBuf};

use chrono::Utc;
use tokio::task::JoinSet;
use tracing::{error, info, warn};

use std::sync::Arc;

use crate::config::{CliOverrides, Route, RunnerConfig};
use crate::error::{Error, Result};
use crate::executor::StageResult;
use crate::git::GitClient;
use crate::github;
use crate::github::GitHubClient;
use crate::isolation;
use crate::notifications;
use crate::planner;
use crate::state::{self, RunRecord, RunStatus, StageStatus};
use crate::workflow::WorkflowMode;

/// A pending work item in the batch queue — either an issue or a PR.
enum PendingSubject {
    Issue(github::IssueCandidate),
    Pr(github::PrCandidate),
}

/// Process a single issue through the full pipeline using the new config model.
#[allow(clippy::too_many_arguments)]
pub async fn process_issue_with_config(
    number: u64,
    repo: &str,
    routes: &IndexMap<String, Route>,
    config: &RunnerConfig,
    state_dir: &Path,
    repo_dir: &Path,
    gh: &dyn GitHubClient,
    git: &dyn GitClient,
) -> Result<RunRecord> {
    process_issue_with_overrides(
        number,
        repo,
        routes,
        config,
        state_dir,
        repo_dir,
        CliOverrides::default(),
        gh,
        git,
    )
    .await
}

/// Process a single issue with per-run CLI overrides (--model, --skill).
///
/// CLI overrides take precedence over all config-file settings:
/// CLI flag > stage config > route config > global config.
#[allow(clippy::too_many_arguments)]
pub async fn process_issue_with_overrides(
    number: u64,
    repo: &str,
    routes: &IndexMap<String, Route>,
    config: &RunnerConfig,
    state_dir: &Path,
    repo_dir: &Path,
    cli_overrides: CliOverrides,
    gh: &dyn GitHubClient,
    git: &dyn GitClient,
) -> Result<RunRecord> {
    info!(repo = repo, issue = number, "processing issue");

    // 1. Fetch the issue.
    let issue = gh.fetch_issue(repo, number).await?;
    info!(
        issue = number,
        title = issue.title,
        labels = ?issue.labels,
        "fetched issue"
    );

    // 2. Match route.
    let (route_name, route) = RunnerConfig::match_route_in(routes, &issue)
        .ok_or_else(|| Error::Triage("no route matches this issue's labels".into()))?;
    info!(issue = number, route = route_name, "matched route");

    // 2b. Parse label overrides (forza:model:*, forza:skill:*).
    let label_overrides = crate::config::LabelOverrides::from_labels(&issue.labels);
    if !label_overrides.is_empty() {
        info!(
            issue = number,
            model = ?label_overrides.model,
            skills = ?label_overrides.skills,
            "label overrides detected"
        );
    }

    // 2a. Security gates (checked before acquiring the lease so rejected issues never get
    //     the in_progress label applied).
    if let Some(ref required) = config.security.require_label
        && !issue.labels.iter().any(|l| l == required)
    {
        return Err(Error::Authorization(format!(
            "issue #{number} is missing required security label '{required}'"
        )));
    }
    info!(
        issue = number,
        authorization_level = config.security.authorization_level,
        "security authorization level"
    );
    if !config.security.allowed_authors.is_empty() {
        if !config
            .security
            .allowed_authors
            .iter()
            .any(|a| a == &issue.author)
        {
            return Err(Error::Authorization(format!(
                "issue #{number} author '{}' is not in allowed_authors",
                issue.author
            )));
        }
    } else {
        // Empty allowed_authors: only the authenticated gh user may trigger runs.
        let authenticated = gh.fetch_authenticated_user().await?;
        if issue.author != authenticated {
            return Err(Error::Authorization(format!(
                "issue #{number} author '{}' does not match authenticated user '{authenticated}'",
                issue.author
            )));
        }
    }

    // 3. Acquire lease.
    let _ = gh
        .add_label(repo, number, &config.global.in_progress_label)
        .await;
    if let Some(ref gate) = config.global.gate_label {
        let _ = gh.remove_label(repo, number, gate).await;
    }

    // 4. Resolve workflow template.
    let template = config
        .resolve_workflow(route.workflow.as_deref().unwrap_or(""))
        .ok_or_else(|| {
            Error::Policy(format!(
                "unknown workflow: {}",
                route.workflow.as_deref().unwrap_or("<none>")
            ))
        })?;
    let branch = config.branch_for_issue(&issue);
    info!(
        issue = number,
        route = route_name,
        workflow = template.name,
        branch = branch,
        stages = template.stages.len(),
        "selected workflow"
    );

    // 5. Create work plan.
    let run_id = state::generate_run_id();
    let plan = planner::create_plan_with_config(
        &issue,
        &template,
        &branch,
        Some((config, repo_dir)),
        &run_id,
    );

    // 6. Set up isolation.
    let worktree_dir = isolation::create_worktree(repo_dir, &branch, ".worktrees", git).await?;
    info!(issue = number, worktree = %worktree_dir.display(), "created worktree");

    // 7. Execute stages.
    let validation = config.effective_validation(route);
    let draft_pr = config.effective_draft_pr(route);

    let mut record = RunRecord::new(&run_id, repo, number, &template.name, &branch);

    // Check hourly cost cap before starting any stages.
    if let Some(cap) = config.global.max_cost_per_hour {
        let spent = state::hourly_cost(state_dir);
        if spent >= cap {
            warn!(
                issue = number,
                spent = spent,
                cap = cap,
                "hourly cost cap exceeded, aborting run"
            );
            record.outcome = Some(state::RouteOutcome::Failed {
                stage: "cost_cap".to_string(),
                error: format!("hourly cost cap exceeded: ${spent:.4} >= ${cap:.4}"),
            });
            record.finish(RunStatus::Failed);
            state::save_run(&record, state_dir)?;
            return Ok(record);
        }
    }

    let ctx = StageContext {
        subject_number: number,
        subject_label: "issue",
        repo,
        branch: &branch,
        run_id: &run_id,
        issue: Some(&issue),
        draft_pr,
        cli_overrides: &cli_overrides,
        label_overrides: &label_overrides,
    };
    let all_succeeded = execute_stages(
        &plan.stages,
        &ctx,
        config,
        route,
        &mut record,
        &worktree_dir,
        gh,
        git,
        validation,
    )
    .await?;

    // 8. Finish.
    let final_status = if all_succeeded {
        RunStatus::Succeeded
    } else {
        RunStatus::Failed
    };
    record.outcome = Some(if all_succeeded {
        if let Some(pr) = record.pr_number {
            state::RouteOutcome::PrCreated { number: pr }
        } else {
            state::RouteOutcome::CommentPosted
        }
    } else {
        let failed_stage = record
            .stages
            .iter()
            .find(|s| s.status == state::StageStatus::Failed);
        state::RouteOutcome::Failed {
            stage: failed_stage
                .map(|s| s.kind_name().to_string())
                .unwrap_or_default(),
            error: failed_stage
                .and_then(|s| s.result.as_ref())
                .map(|r| r.output.chars().take(200).collect())
                .unwrap_or_default(),
        }
    });
    record.finish(final_status);
    state::save_run(&record, state_dir)?;
    info!(issue = number, run_id = record.run_id, status = ?final_status, outcome = ?record.outcome, cost = record.total_cost_usd, "run complete");

    // Fire notifications (best-effort; errors are logged, never fatal).
    if let Some(notif_config) = config.global.notifications.as_ref() {
        notifications::notify_run_complete(notif_config, &record).await;
    }

    // 9. Update lease labels.
    let _ = gh
        .remove_label(repo, number, &config.global.in_progress_label)
        .await;
    if all_succeeded {
        let _ = gh
            .add_label(repo, number, &config.global.complete_label)
            .await;
    } else {
        let _ = gh
            .add_label(repo, number, &config.global.failed_label)
            .await;
    }

    // 10. Cleanup (always — prevents stale worktrees blocking retries).
    if let Err(e) = isolation::remove_worktree(repo_dir, &worktree_dir, true, git).await {
        warn!(error = %e, "failed to clean worktree (non-fatal)");
    }

    Ok(record)
}

/// Process a single PR through the full pipeline using the new config model.
#[allow(clippy::too_many_arguments)]
pub async fn process_pr_with_config(
    number: u64,
    repo: &str,
    routes: &IndexMap<String, Route>,
    config: &RunnerConfig,
    state_dir: &Path,
    repo_dir: &Path,
    gh: &dyn GitHubClient,
    git: &dyn GitClient,
) -> Result<RunRecord> {
    process_pr_with_overrides(
        number,
        repo,
        routes,
        config,
        state_dir,
        repo_dir,
        CliOverrides::default(),
        gh,
        git,
    )
    .await
}

/// Process a single PR with per-run CLI overrides (--model, --skill).
///
/// CLI overrides take precedence over all config-file settings:
/// CLI flag > stage config > route config > global config.
#[allow(clippy::too_many_arguments)]
pub async fn process_pr_with_overrides(
    number: u64,
    repo: &str,
    routes: &IndexMap<String, Route>,
    config: &RunnerConfig,
    state_dir: &Path,
    repo_dir: &Path,
    cli_overrides: CliOverrides,
    gh: &dyn GitHubClient,
    git: &dyn GitClient,
) -> Result<RunRecord> {
    info!(repo = repo, pr = number, "processing PR");

    // 1. Fetch the PR.
    let pr = gh.fetch_pr(repo, number).await?;
    info!(
        pr = number,
        title = pr.title,
        labels = ?pr.labels,
        "fetched PR"
    );

    // 2. Match route.
    let (route_name, route) = RunnerConfig::match_pr_route_in(routes, &pr)
        .ok_or_else(|| Error::Triage("no route matches this PR's labels".into()))?;

    // 2b. Parse label overrides.
    let label_overrides = crate::config::LabelOverrides::from_labels(&pr.labels);
    if !label_overrides.is_empty() {
        info!(
            pr = number,
            model = ?label_overrides.model,
            skills = ?label_overrides.skills,
            "label overrides detected"
        );
    }
    info!(pr = number, route = route_name, "matched route");

    // 3. Acquire lease.
    let _ = gh
        .add_pr_label(repo, number, &config.global.in_progress_label)
        .await;
    if let Some(ref gate) = config.global.gate_label {
        let _ = gh.remove_pr_label(repo, number, gate).await;
    }

    // 4. Resolve workflow template.
    let template = config
        .resolve_workflow(route.workflow.as_deref().unwrap_or(""))
        .ok_or_else(|| {
            Error::Policy(format!(
                "unknown workflow: {}",
                route.workflow.as_deref().unwrap_or("<none>")
            ))
        })?;
    let branch = RunnerConfig::branch_for_pr(&pr);
    info!(
        pr = number,
        route = route_name,
        workflow = template.name,
        branch = branch,
        stages = template.stages.len(),
        "selected workflow"
    );

    // 5. Create work plan.
    let run_id = state::generate_run_id();
    let plan = planner::create_pr_plan_with_config(
        &pr,
        &template,
        &branch,
        Some((config, repo_dir)),
        &run_id,
    );

    // 6. Set up isolation (skip worktree for read-only or agentless-only workflows).
    let needs_worktree = !matches!(template.name.as_str(), "pr-review" | "pr-merge");
    let worktree_dir = if needs_worktree {
        let dir = isolation::create_worktree(repo_dir, &branch, ".worktrees", git).await?;
        info!(pr = number, worktree = %dir.display(), "created worktree");
        dir
    } else {
        repo_dir.to_path_buf()
    };

    // 7. Execute stages.
    let validation = config.effective_validation(route);
    let mut record = RunRecord::new_for_pr(&run_id, repo, number, &template.name, &branch);

    let ctx = StageContext {
        subject_number: number,
        subject_label: "pr",
        repo,
        branch: &branch,
        run_id: &run_id,
        issue: None,
        draft_pr: false,
        cli_overrides: &cli_overrides,
        label_overrides: &label_overrides,
    };
    let all_succeeded = execute_stages(
        &plan.stages,
        &ctx,
        config,
        route,
        &mut record,
        &worktree_dir,
        gh,
        git,
        validation,
    )
    .await?;

    // 8. Finish.
    let final_status = if all_succeeded {
        RunStatus::Succeeded
    } else {
        RunStatus::Failed
    };
    record.outcome = Some(if all_succeeded {
        state::RouteOutcome::PrUpdated { number }
    } else {
        let failed_stage = record
            .stages
            .iter()
            .find(|s| s.status == state::StageStatus::Failed);
        state::RouteOutcome::Failed {
            stage: failed_stage
                .map(|s| s.kind_name().to_string())
                .unwrap_or_default(),
            error: failed_stage
                .and_then(|s| s.result.as_ref())
                .map(|r| r.output.chars().take(200).collect())
                .unwrap_or_default(),
        }
    });
    record.finish(final_status);
    state::save_run(&record, state_dir)?;
    info!(pr = number, run_id = record.run_id, status = ?final_status, outcome = ?record.outcome, cost = record.total_cost_usd, "run complete");

    // Fire notifications.
    if let Some(notif_config) = config.global.notifications.as_ref() {
        notifications::notify_run_complete(notif_config, &record).await;
    }

    // 9. Update lease labels.
    let _ = gh
        .remove_pr_label(repo, number, &config.global.in_progress_label)
        .await;
    if all_succeeded {
        let _ = gh
            .add_pr_label(repo, number, &config.global.complete_label)
            .await;
    } else {
        let _ = gh
            .add_pr_label(repo, number, &config.global.failed_label)
            .await;
    }

    // 10. Cleanup (always — prevents stale worktrees blocking retries).
    if needs_worktree
        && let Err(e) = isolation::remove_worktree(repo_dir, &worktree_dir, true, git).await
    {
        warn!(error = %e, "failed to clean worktree (non-fatal)");
    }

    Ok(record)
}

/// Process a single PR through one reactive maintenance cycle.
///
/// Evaluates each template stage's condition and runs the first stage whose
/// condition exits 0. Only one stage executes per cycle (the reactive dispatch
/// model). Returns the run record regardless of whether a stage ran.
#[allow(clippy::too_many_arguments)]
pub async fn process_reactive_pr(
    pr_number: u64,
    repo: &str,
    route_name: &str,
    routes: &IndexMap<String, Route>,
    config: &RunnerConfig,
    state_dir: &Path,
    repo_dir: &Path,
    gh: &dyn GitHubClient,
    git: &dyn GitClient,
) -> Result<RunRecord> {
    info!(
        repo = repo,
        pr = pr_number,
        "processing PR in reactive mode"
    );

    // 1. Fetch the PR.
    let pr = gh.fetch_pr(repo, pr_number).await?;
    info!(pr = pr_number, title = pr.title, "fetched PR");

    // 1a. Acquire in-progress lease.
    let _ = gh
        .add_pr_label(repo, pr_number, &config.global.in_progress_label)
        .await;

    // 1b. Parse label overrides.
    let label_overrides = crate::config::LabelOverrides::from_labels(&pr.labels);

    // 2. Resolve route and template.
    let route = routes
        .get(route_name)
        .ok_or_else(|| Error::Policy(format!("route not found: {route_name}")))?;
    let template = config
        .resolve_workflow(route.workflow.as_deref().unwrap_or(""))
        .ok_or_else(|| {
            Error::Policy(format!(
                "unknown workflow: {}",
                route.workflow.as_deref().unwrap_or("<none>")
            ))
        })?;

    if template.mode != WorkflowMode::Reactive {
        return Err(Error::Policy(format!(
            "workflow '{}' is not in reactive mode",
            template.name
        )));
    }

    let branch = pr.head_branch.clone();
    info!(
        pr = pr_number,
        route = route_name,
        workflow = template.name,
        branch = branch,
        "selected reactive workflow"
    );

    // 3. Set up isolation.
    let worktree_dir = isolation::create_worktree(repo_dir, &branch, ".worktrees", git).await?;
    info!(pr = pr_number, worktree = %worktree_dir.display(), "created worktree");

    // 4. Create run record.
    let run_id = state::generate_run_id();
    let mut record = RunRecord::new(&run_id, repo, pr_number, &template.name, &branch);

    // 5. Reactive dispatch: evaluate each stage condition, run the first that passes.
    let mut stage_ran = false;
    for stage in &template.stages {
        // In reactive mode, condition exit 0 = run this stage.
        let should_run = if let Some(ref condition) = stage.condition {
            tokio::process::Command::new("sh")
                .args(["-c", condition])
                .current_dir(&worktree_dir)
                .env("FORZA_PR_NUMBER", pr_number.to_string())
                .output()
                .await
                .map(|o| o.status.success())
                .unwrap_or(false)
        } else {
            true // No condition → always eligible.
        };

        if !should_run {
            info!(
                pr = pr_number,
                stage = stage.kind.name(),
                "reactive stage condition not met, trying next"
            );
            continue;
        }

        info!(
            pr = pr_number,
            stage = stage.kind.name(),
            agentless = stage.agentless,
            "reactive stage selected"
        );

        let planned = build_pr_planned_stage(stage, &pr, &run_id, &template.stages);

        if planned.agentless {
            let Some(command) = planned.command.as_deref() else {
                return Err(Error::Executor(format!(
                    "agentless stage '{}' has no command",
                    stage.kind.name()
                )));
            };
            let (success, output_text, duration) =
                run_agentless_stage(command, &worktree_dir, Some(pr_number)).await;
            info!(
                pr = pr_number,
                stage = planned.kind_name(),
                success,
                duration = format!("{:.1}s", duration.as_secs_f64()),
                "reactive agentless stage complete"
            );
            record.record_stage(
                planned.kind,
                if success {
                    StageStatus::Succeeded
                } else {
                    StageStatus::Failed
                },
                StageResult {
                    stage: planned.kind_name().to_string(),
                    success,
                    duration_secs: duration.as_secs_f64(),
                    cost_usd: None,
                    output: output_text,
                    files_modified: None,
                },
            );
        } else {
            match run_agent_stage(
                &planned,
                config,
                route,
                &worktree_dir,
                &CliOverrides::default(),
                &label_overrides,
            )
            .await
            {
                Ok(result) => {
                    info!(
                        pr = pr_number,
                        stage = planned.kind_name(),
                        success = result.success,
                        duration = format!("{:.1}s", result.duration_secs),
                        "reactive stage complete"
                    );
                    record.record_stage(
                        planned.kind,
                        if result.success {
                            StageStatus::Succeeded
                        } else {
                            StageStatus::Failed
                        },
                        result,
                    );
                }
                Err(e) => {
                    error!(pr = pr_number, stage = planned.kind_name(), error = %e, "reactive stage error");
                    record.record_stage(
                        planned.kind,
                        StageStatus::Failed,
                        StageResult {
                            stage: planned.kind_name().to_string(),
                            success: false,
                            duration_secs: 0.0,
                            cost_usd: None,
                            output: e.to_string(),
                            files_modified: None,
                        },
                    );
                }
            }
        }

        stage_ran = true;
        break; // Reactive mode: only one stage per cycle.
    }

    if !stage_ran {
        info!(pr = pr_number, "no reactive stage condition met this cycle");
    }

    // 6. Finish.
    let all_succeeded = !stage_ran
        || record
            .stages
            .iter()
            .all(|s| s.status == StageStatus::Succeeded);
    let final_status = if all_succeeded {
        RunStatus::Succeeded
    } else {
        RunStatus::Failed
    };
    record.outcome = Some(if !stage_ran {
        state::RouteOutcome::NothingToDo
    } else if all_succeeded {
        state::RouteOutcome::PrUpdated { number: pr_number }
    } else {
        let failed_stage = record
            .stages
            .iter()
            .find(|s| s.status == state::StageStatus::Failed);
        state::RouteOutcome::Failed {
            stage: failed_stage
                .map(|s| s.kind_name().to_string())
                .unwrap_or_default(),
            error: failed_stage
                .and_then(|s| s.result.as_ref())
                .map(|r| r.output.chars().take(200).collect())
                .unwrap_or_default(),
        }
    });
    record.finish(final_status);
    state::save_run(&record, state_dir)?;
    info!(pr = pr_number, run_id = record.run_id, status = ?final_status, outcome = ?record.outcome, "reactive PR run complete");

    // Release in-progress lease.
    let _ = gh
        .remove_pr_label(repo, pr_number, &config.global.in_progress_label)
        .await;

    // Fire notifications (best-effort).
    if let Some(notif_config) = config.global.notifications.as_ref() {
        notifications::notify_run_complete(notif_config, &record).await;
    }

    // 7. Cleanup (always — prevents stale worktrees blocking retries).
    if let Err(e) = isolation::remove_worktree(repo_dir, &worktree_dir, true, git).await {
        warn!(error = %e, "failed to clean worktree (non-fatal)");
    }

    Ok(record)
}

/// Process all eligible issues for a single repo using its route table.
#[allow(clippy::too_many_arguments)]
pub async fn process_batch_for_repo(
    repo: &str,
    config: &RunnerConfig,
    state_dir: &Path,
    repo_dir: &Path,
    routes: &IndexMap<String, Route>,
    cancel: &tokio::sync::watch::Receiver<bool>,
    gh: Arc<dyn GitHubClient>,
    git: Arc<dyn GitClient>,
) -> Vec<RunRecord> {
    // Recover stale leases before processing new work.
    let stale_timeout = std::time::Duration::from_secs(config.global.stale_lease_timeout);
    let now = Utc::now();
    let surviving_in_progress: Vec<crate::github::IssueCandidate> = match gh
        .fetch_issues_with_label(repo, &config.global.in_progress_label)
        .await
    {
        Ok(in_progress) => {
            let mut survivors = Vec::new();
            for issue in in_progress {
                let lease_age = chrono::DateTime::parse_from_rfc3339(&issue.updated_at)
                    .ok()
                    .and_then(|t| (now - t.with_timezone(&Utc)).to_std().ok());
                if lease_age.is_some_and(|age| age >= stale_timeout) {
                    warn!(
                        issue = issue.number,
                        age_secs = lease_age.unwrap().as_secs(),
                        "removing stale in-progress lease"
                    );
                    let _ = gh
                        .remove_label(repo, issue.number, &config.global.in_progress_label)
                        .await;
                } else {
                    survivors.push(issue);
                }
            }
            survivors
        }
        Err(e) => {
            warn!(error = %e, "failed to fetch in-progress issues for stale lease check");
            vec![]
        }
    };

    // Fetch issues with the gate label if configured.
    let labels = config
        .global
        .gate_label
        .as_deref()
        .map(|l| vec![l.to_string()])
        .unwrap_or_default();

    let issues = match gh.fetch_eligible_issues(repo, &labels, 10).await {
        Ok(issues) => issues,
        Err(e) => {
            error!(error = %e, "failed to fetch issues");
            return vec![];
        }
    };

    info!(repo = repo, count = issues.len(), "found eligible issues");

    // Pre-filter: keep only issues that match a route and are within their schedule window.
    let now = chrono::Utc::now();
    let mut pending: VecDeque<(String, PendingSubject)> = issues
        .into_iter()
        .filter_map(|issue| {
            if let Some((route_name, route)) = RunnerConfig::match_route_in(routes, &issue) {
                if let Some(schedule) = &route.schedule
                    && !schedule.is_active(now)
                {
                    tracing::debug!(
                        issue = issue.number,
                        route = route_name,
                        "outside schedule window, skipping"
                    );
                    return None;
                }
                Some((route_name.to_string(), PendingSubject::Issue(issue)))
            } else {
                warn!(
                    issue = issue.number,
                    labels = ?issue.labels,
                    "no route matches issue labels, skipping — add a matching route to config"
                );
                None
            }
        })
        .collect();

    // Discover PRs for label-based "pr"-type routes.
    let label_pr_routes: Vec<(&str, &Route)> = routes
        .iter()
        .filter(|(_, r)| {
            r.route_type == crate::config::SubjectType::Pr
                && r.label.is_some()
                && r.condition.is_none()
        })
        .map(|(name, route)| (name.as_str(), route))
        .collect();

    for (pr_route_name, pr_route) in &label_pr_routes {
        if let Some(schedule) = &pr_route.schedule
            && !schedule.is_active(now)
        {
            tracing::debug!(
                route = pr_route_name,
                "PR route outside schedule window, skipping"
            );
            continue;
        }
        let label = pr_route.label.as_deref().unwrap();
        match gh.fetch_prs_with_label(repo, label).await {
            Ok(prs) => {
                // Filter out PRs that are already complete, needs-human, or in-progress.
                let actionable: Vec<_> = prs
                    .into_iter()
                    .filter(|pr| {
                        !pr.labels.iter().any(|l| {
                            l == &config.global.complete_label
                                || l == &config.global.in_progress_label
                                || l == "forza:needs-human"
                        })
                    })
                    .collect();
                info!(
                    repo = repo,
                    route = pr_route_name,
                    count = actionable.len(),
                    "found eligible PRs"
                );
                for pr in actionable {
                    pending.push_back((pr_route_name.to_string(), PendingSubject::Pr(pr)));
                }
            }
            Err(e) => {
                warn!(route = pr_route_name, error = %e, "failed to fetch PRs for route");
            }
        }
    }

    // Discover PRs for condition-based routes.
    let condition_routes: Vec<(&str, &Route)> = routes
        .iter()
        .filter(|(_, r)| r.route_type == crate::config::SubjectType::Pr && r.condition.is_some())
        .map(|(name, route)| (name.as_str(), route))
        .collect();

    if !condition_routes.is_empty() {
        let all_prs = match gh.fetch_all_open_prs(repo, 100).await {
            Ok(prs) => {
                info!(
                    repo = repo,
                    count = prs.len(),
                    "fetched open PRs for condition route evaluation"
                );
                prs
            }
            Err(e) => {
                warn!(error = %e, "failed to fetch open PRs for condition routes");
                vec![]
            }
        };

        let branch_prefix = config
            .global
            .branch_pattern
            .split('{')
            .next()
            .unwrap_or("automation/");

        for (route_name, route) in &condition_routes {
            let condition = route.condition.as_ref().unwrap();
            let mut matched: usize = 0;
            for pr in &all_prs {
                // Skip PRs already in progress or marked needs-human.
                if pr
                    .labels
                    .iter()
                    .any(|l| l == &config.global.in_progress_label || l == "forza:needs-human")
                {
                    tracing::debug!(
                        pr = pr.number,
                        route = route_name,
                        "skipping PR: in-progress or needs-human label"
                    );
                    continue;
                }

                // Apply scope filter.
                if route.scope == crate::config::ConditionScope::ForzaOwned
                    && !pr.head_branch.starts_with(branch_prefix)
                {
                    tracing::debug!(
                        pr = pr.number,
                        route = route_name,
                        "skipping PR: outside forza_owned scope"
                    );
                    continue;
                }

                if !condition.matches(pr) {
                    tracing::debug!(
                        pr = pr.number,
                        route = route_name,
                        condition = ?condition,
                        "skipping PR: condition not matched"
                    );
                    continue;
                }

                // Check retry budget and exponential backoff.
                let workflow_name = route.workflow.as_deref().unwrap_or("");
                let prior_fails =
                    state::count_failed_runs_for_subject(pr.number, workflow_name, state_dir);
                if let Some(max) = route.max_retries
                    && prior_fails >= max
                {
                    warn!(
                        pr = pr.number,
                        route = route_name,
                        prior_fails = prior_fails,
                        max_retries = max,
                        "retry budget exhausted, applying needs-human label"
                    );
                    let _ = gh.add_pr_label(repo, pr.number, "forza:needs-human").await;
                    let _ = gh
                        .comment_on_pr(
                            repo,
                            pr.number,
                            &format!(
                                "Retry budget exhausted for route `{route_name}` \
                             ({prior_fails}/{max} attempts). Applying `forza:needs-human` \
                             for manual review."
                            ),
                        )
                        .await;
                    continue;
                }

                // Exponential backoff: if there are prior failures, wait before retrying.
                // backoff_secs = poll_interval * 2^(prior_fails - 1), capped at 2^6 = 64x.
                if prior_fails > 0
                    && let Some(last) = state::find_last_completed_run_for_subject(
                        pr.number,
                        workflow_name,
                        state_dir,
                    )
                    && let Some(completed_at) = last.completed_at
                {
                    let exponent = (prior_fails - 1).min(6) as u32;
                    let backoff_secs = route.poll_interval * 2u64.pow(exponent);
                    let elapsed = chrono::Utc::now()
                        .signed_duration_since(completed_at)
                        .num_seconds()
                        .max(0) as u64;
                    if elapsed < backoff_secs {
                        tracing::debug!(
                            pr = pr.number,
                            route = route_name,
                            prior_fails = prior_fails,
                            elapsed_secs = elapsed,
                            backoff_secs = backoff_secs,
                            "skipping PR: within exponential backoff window"
                        );
                        continue;
                    }
                }

                info!(
                    pr = pr.number,
                    route = route_name,
                    condition = ?condition,
                    "condition matched, queuing PR"
                );
                pending.push_back((route_name.to_string(), PendingSubject::Pr(pr.clone())));
                matched += 1;
            }
            info!(
                repo = repo,
                route = route_name,
                count = matched,
                "found eligible PRs for condition route"
            );
        }
    }

    let max_concurrency = config.global.max_concurrency;
    // Seed active counts from non-stale in-progress issues so that limits
    // are respected correctly after a restart.
    let mut total_active: usize = 0;
    let mut active_per_route: HashMap<String, usize> = HashMap::new();
    for issue in &surviving_in_progress {
        total_active += 1;
        if let Some((route_name, _)) = RunnerConfig::match_route_in(routes, issue) {
            *active_per_route.entry(route_name.to_string()).or_insert(0) += 1;
        }
    }
    if total_active > 0 {
        info!(
            total_active,
            "seeded active counts from surviving in-progress issues"
        );
    }
    let mut join_set: JoinSet<(String, Result<RunRecord>)> = JoinSet::new();
    let mut records = Vec::new();

    loop {
        if *cancel.borrow() {
            info!("cancellation requested, stopping batch");
            break;
        }

        // Try to fill available slots from pending queue.
        let mut deferred: VecDeque<(String, PendingSubject)> = VecDeque::new();
        while let Some((route_name, subject)) = pending.pop_front() {
            let route_active = *active_per_route.get(&route_name).unwrap_or(&0);
            let route_limit = routes.get(&route_name).map(|r| r.concurrency).unwrap_or(1);

            if total_active < max_concurrency && route_active < route_limit {
                total_active += 1;
                *active_per_route.entry(route_name.clone()).or_insert(0) += 1;

                let config_clone = config.clone();
                let state_dir_owned: PathBuf = state_dir.to_path_buf();
                let repo_dir_owned: PathBuf = repo_dir.to_path_buf();
                let repo_owned = repo.to_string();
                let routes_clone = routes.clone();
                let gh_clone = gh.clone();
                let git_clone = git.clone();
                match subject {
                    PendingSubject::Issue(issue) => {
                        let issue_number = issue.number;
                        join_set.spawn(async move {
                            let result = process_issue_with_config(
                                issue_number,
                                &repo_owned,
                                &routes_clone,
                                &config_clone,
                                &state_dir_owned,
                                &repo_dir_owned,
                                &*gh_clone,
                                &*git_clone,
                            )
                            .await;
                            (route_name, result)
                        });
                    }
                    PendingSubject::Pr(pr) => {
                        let pr_number = pr.number;
                        let route_name_for_reactive = route_name.clone();
                        let is_reactive = routes
                            .get(&route_name)
                            .and_then(|r| r.workflow.as_deref())
                            .and_then(|wf| config.resolve_workflow(wf))
                            .is_some_and(|t| t.mode == crate::workflow::WorkflowMode::Reactive);
                        join_set.spawn(async move {
                            let result = if is_reactive {
                                process_reactive_pr(
                                    pr_number,
                                    &repo_owned,
                                    &route_name_for_reactive,
                                    &routes_clone,
                                    &config_clone,
                                    &state_dir_owned,
                                    &repo_dir_owned,
                                    &*gh_clone,
                                    &*git_clone,
                                )
                                .await
                            } else {
                                process_pr_with_config(
                                    pr_number,
                                    &repo_owned,
                                    &routes_clone,
                                    &config_clone,
                                    &state_dir_owned,
                                    &repo_dir_owned,
                                    &*gh_clone,
                                    &*git_clone,
                                )
                                .await
                            };
                            (route_name, result)
                        });
                    }
                }
            } else if total_active >= max_concurrency {
                info!(
                    route = route_name,
                    active = total_active,
                    max = max_concurrency,
                    "global concurrency limit reached, deferring"
                );
                deferred.push_back((route_name, subject));
            } else {
                info!(
                    route = route_name,
                    active = route_active,
                    limit = route_limit,
                    "per-route concurrency limit reached, deferring"
                );
                deferred.push_back((route_name, subject));
            }
        }
        pending = deferred;

        if pending.is_empty() && join_set.is_empty() {
            break;
        }

        // Wait for one task to finish, then update counters.
        if let Some(join_result) = join_set.join_next().await {
            match join_result {
                Ok((route_name, Ok(record))) => {
                    total_active = total_active.saturating_sub(1);
                    if let Some(c) = active_per_route.get_mut(&route_name) {
                        *c = c.saturating_sub(1);
                    }
                    records.push(record);
                }
                Ok((route_name, Err(e))) => {
                    total_active = total_active.saturating_sub(1);
                    if let Some(c) = active_per_route.get_mut(&route_name) {
                        *c = c.saturating_sub(1);
                    }
                    warn!(route = route_name, error = %e, "failed to process issue");
                }
                Err(e) => {
                    total_active = total_active.saturating_sub(1);
                    warn!(error = %e, "task join error");
                }
            }
        }
    }

    // Drain any still-running tasks (e.g. after cancellation).
    while let Some(join_result) = join_set.join_next().await {
        match join_result {
            Ok((_, Ok(record))) => records.push(record),
            Ok((route_name, Err(e))) => {
                warn!(route = route_name, error = %e, "failed to process issue");
            }
            Err(e) => warn!(error = %e, "task join error"),
        }
    }

    // Process PR-type routes with reactive dispatch (only reactive-mode workflows).
    let pr_routes: Vec<(String, String)> = routes
        .iter()
        .filter(|(_, r)| {
            r.route_type == crate::config::SubjectType::Pr
                && r.label.is_some()
                && r.workflow.as_deref().is_some_and(|wf| {
                    config
                        .resolve_workflow(wf)
                        .is_some_and(|t| t.mode == crate::workflow::WorkflowMode::Reactive)
                })
        })
        .map(|(name, r)| (name.clone(), r.label.clone().unwrap()))
        .collect();

    if !pr_routes.is_empty() && !*cancel.borrow() {
        let mut pr_join_set: JoinSet<(String, Result<RunRecord>)> = JoinSet::new();

        for (route_name, label) in pr_routes {
            if *cancel.borrow() {
                break;
            }
            let prs = match gh.fetch_eligible_prs(repo, &[label], 10).await {
                Ok(prs) => prs,
                Err(e) => {
                    error!(error = %e, route = route_name, "failed to fetch eligible PRs");
                    continue;
                }
            };

            info!(
                repo = repo,
                route = route_name,
                count = prs.len(),
                "found eligible PRs"
            );

            for pr in prs {
                if *cancel.borrow() {
                    break;
                }
                let config_clone = config.clone();
                let state_dir_owned = state_dir.to_path_buf();
                let repo_dir_owned = repo_dir.to_path_buf();
                let pr_number = pr.number;
                let repo_owned = repo.to_string();
                let route_name_clone = route_name.clone();
                let routes_clone = routes.clone();
                let gh_clone = gh.clone();
                let git_clone = git.clone();
                pr_join_set.spawn(async move {
                    let result = process_reactive_pr(
                        pr_number,
                        &repo_owned,
                        &route_name_clone,
                        &routes_clone,
                        &config_clone,
                        &state_dir_owned,
                        &repo_dir_owned,
                        &*gh_clone,
                        &*git_clone,
                    )
                    .await;
                    (route_name_clone, result)
                });
            }
        }

        while let Some(join_result) = pr_join_set.join_next().await {
            match join_result {
                Ok((_, Ok(record))) => records.push(record),
                Ok((route_name, Err(e))) => {
                    warn!(route = route_name, error = %e, "failed to process PR");
                }
                Err(e) => warn!(error = %e, "PR task join error"),
            }
        }
    }

    records
}

/// Process all eligible issues across all configured repos.
///
/// Iterates repos sequentially; within each repo issues run with per-route
/// concurrency. `repo_dir` is used as a fallback when a repo entry has no
/// explicit local path configured.
pub async fn process_batch_with_config(
    config: &RunnerConfig,
    state_dir: &Path,
    repo_dir: &Path,
    cancel: &tokio::sync::watch::Receiver<bool>,
    gh: Arc<dyn GitHubClient>,
    git: Arc<dyn GitClient>,
) -> Vec<RunRecord> {
    let mut all_records = Vec::new();
    for (repo, repo_dir_opt, routes) in config.iter_repos() {
        if *cancel.borrow() {
            break;
        }
        let rd = repo_dir_opt
            .map(PathBuf::from)
            .unwrap_or_else(|| repo_dir.to_path_buf());
        let mut records = process_batch_for_repo(
            repo,
            config,
            state_dir,
            &rd,
            routes,
            cancel,
            gh.clone(),
            git.clone(),
        )
        .await;
        all_records.append(&mut records);
    }
    all_records
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::config::RunnerConfig;
    use crate::github::IssueCandidate;

    fn make_issue(number: u64, labels: Vec<&str>) -> IssueCandidate {
        IssueCandidate {
            number,
            repo: "owner/repo".into(),
            title: "test issue".into(),
            body: String::new(),
            labels: labels.into_iter().map(String::from).collect(),
            state: "open".into(),
            created_at: String::new(),
            updated_at: String::new(),
            is_assigned: false,
            html_url: String::new(),
            author: String::new(),
            comments: vec![],
        }
    }

    fn two_route_config(max_concurrency: usize, bug_concurrency: usize) -> RunnerConfig {
        let toml = format!(
            r#"
[global]
repo = "owner/repo"
max_concurrency = {max_concurrency}

[routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"
concurrency = {bug_concurrency}

[routes.features]
type = "issue"
label = "enhancement"
workflow = "feature"
concurrency = 1
"#
        );
        toml::from_str(&toml).unwrap()
    }

    /// Simulate one pass of the scheduling inner loop. Returns (scheduled, deferred).
    fn simulate_scheduling_pass(
        pending: &[(String, IssueCandidate)],
        total_active: usize,
        active_per_route: &HashMap<String, usize>,
        max_concurrency: usize,
        config: &RunnerConfig,
    ) -> (usize, usize) {
        let mut scheduled = 0usize;
        let mut deferred = 0usize;
        let mut ta = total_active;
        let mut apr = active_per_route.clone();

        for (route_name, _issue) in pending {
            let route_active = *apr.get(route_name).unwrap_or(&0);
            let route_limit = config
                .routes
                .get(route_name)
                .map(|r| r.concurrency)
                .unwrap_or(1);

            if ta < max_concurrency && route_active < route_limit {
                ta += 1;
                *apr.entry(route_name.clone()).or_insert(0) += 1;
                scheduled += 1;
            } else {
                deferred += 1;
            }
        }
        (scheduled, deferred)
    }

    #[test]
    fn seed_active_counts_from_surviving_in_progress() {
        let config = two_route_config(5, 2);
        let in_progress = vec![
            make_issue(1, vec!["bug"]),
            make_issue(2, vec!["bug"]),
            make_issue(3, vec!["enhancement"]),
        ];

        let mut total_active = 0usize;
        let mut active_per_route: HashMap<String, usize> = HashMap::new();
        for issue in &in_progress {
            total_active += 1;
            if let Some((route_name, _)) = config.match_route(issue) {
                *active_per_route.entry(route_name.to_string()).or_insert(0) += 1;
            }
        }

        assert_eq!(total_active, 3);
        assert_eq!(active_per_route["bugfix"], 2);
        assert_eq!(active_per_route["features"], 1);
    }

    #[test]
    fn global_cap_defers_issue_when_at_limit() {
        let config = two_route_config(2, 5);
        let pending = vec![("bugfix".to_string(), make_issue(10, vec!["bug"]))];
        let apr = HashMap::new();
        // total_active == max_concurrency — should defer
        let (scheduled, deferred) = simulate_scheduling_pass(&pending, 2, &apr, 2, &config);
        assert_eq!(scheduled, 0);
        assert_eq!(deferred, 1);
    }

    #[test]
    fn per_route_cap_defers_issue_when_at_limit() {
        let config = two_route_config(10, 1); // bugfix concurrency = 1
        let pending = vec![("bugfix".to_string(), make_issue(10, vec!["bug"]))];
        // bugfix already has 1 active — at its per-route limit
        let mut apr = HashMap::new();
        apr.insert("bugfix".to_string(), 1usize);
        let (scheduled, deferred) = simulate_scheduling_pass(&pending, 1, &apr, 10, &config);
        assert_eq!(scheduled, 0);
        assert_eq!(deferred, 1);
    }

    #[test]
    fn scheduling_resumes_after_slot_frees() {
        // max_concurrency=2 with 3 pending bugfix issues.
        let config = two_route_config(2, 3);
        let three_pending = vec![
            ("bugfix".to_string(), make_issue(1, vec!["bug"])),
            ("bugfix".to_string(), make_issue(2, vec!["bug"])),
            ("bugfix".to_string(), make_issue(3, vec!["bug"])),
        ];
        let apr = HashMap::new();

        // First pass: 2 slots available — first two scheduled, third deferred.
        let (scheduled, deferred) = simulate_scheduling_pass(&three_pending, 0, &apr, 2, &config);
        assert_eq!(scheduled, 2);
        assert_eq!(deferred, 1);

        // One task finishes (total_active drops from 2 to 1).
        // Second pass with remaining deferred issue — slot is available.
        let one_pending = vec![("bugfix".to_string(), make_issue(3, vec!["bug"]))];
        let mut apr_after = HashMap::new();
        apr_after.insert("bugfix".to_string(), 1usize); // one still running
        let (scheduled, deferred) =
            simulate_scheduling_pass(&one_pending, 1, &apr_after, 2, &config);
        assert_eq!(scheduled, 1);
        assert_eq!(deferred, 0);
    }

    #[test]
    fn build_pr_body_includes_stage_table_and_breadcrumbs() {
        use crate::executor::StageResult;
        use crate::state::{RunRecord, StageStatus};
        use crate::workflow::StageKind;

        let issue = IssueCandidate {
            number: 42,
            repo: "owner/repo".into(),
            title: "Add feature X".into(),
            body: String::new(),
            labels: vec![],
            state: "open".into(),
            created_at: String::new(),
            updated_at: String::new(),
            is_assigned: false,
            html_url: "https://github.com/owner/repo/issues/42".into(),
            author: String::new(),
            comments: vec![],
        };

        let mut record = RunRecord::new("run-test", "owner/repo", 42, "feature", "feat/42");
        record.record_stage(
            StageKind::Plan,
            StageStatus::Succeeded,
            StageResult {
                stage: "plan".into(),
                success: true,
                duration_secs: 10.5,
                cost_usd: Some(0.05),
                output: String::new(),
                files_modified: None,
            },
        );
        record.record_stage(
            StageKind::Implement,
            StageStatus::Succeeded,
            StageResult {
                stage: "implement".into(),
                success: true,
                duration_secs: 45.0,
                cost_usd: Some(0.20),
                output: String::new(),
                files_modified: None,
            },
        );

        let body = super::build_pr_body(
            &issue,
            &record,
            "Plan summary here.",
            "Review notes here.",
            "src/lib.rs | 10 ++--\n1 file changed",
        );

        assert!(body.contains("## Summary"));
        assert!(body.contains("[#42](https://github.com/owner/repo/issues/42)"));
        assert!(body.contains("Add feature X"));
        assert!(body.contains("## Stages"));
        assert!(body.contains("| plan | succeeded | 10.5s | $0.0500 |"));
        assert!(body.contains("| implement | succeeded | 45.0s | $0.2000 |"));
        assert!(body.contains("**Total cost:** $0.2500"));
        assert!(body.contains("## Files changed"));
        assert!(body.contains("src/lib.rs | 10 ++--"));
        assert!(body.contains("## Plan"));
        assert!(body.contains("Plan summary here."));
        assert!(body.contains("## Review"));
        assert!(body.contains("Review notes here."));
        assert!(body.contains("Closes #42"));
    }

    #[test]
    fn build_pr_body_empty_breadcrumbs_and_diff() {
        use crate::state::RunRecord;

        let issue = make_issue(7, vec![]);
        let record = RunRecord::new("run-empty", "owner/repo", 7, "bug", "fix/7");

        let body = super::build_pr_body(&issue, &record, "", "", "");

        assert!(body.contains("## Summary"));
        assert!(body.contains("#7"));
        assert!(body.contains("## Stages"));
        assert!(!body.contains("## Files changed"));
        assert!(!body.contains("## Plan"));
        assert!(!body.contains("## Review"));
        assert!(!body.contains("Total cost"));
        assert!(body.contains("Closes #7"));
    }
}
