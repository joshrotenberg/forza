//! Orchestrator — the main pipeline that processes a single issue end-to-end.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use chrono::Utc;
use tokio::task::JoinSet;
use tracing::{error, info, warn};

use crate::config::RunnerConfig;
use crate::error::{Error, Result};
use crate::executor::{AgentAdapter, ClaudeAdapter, StageResult};
use crate::github;
use crate::isolation;
use crate::planner;
use crate::policy::RepoPolicy;
use crate::state::{self, RunRecord, RunStatus, StageStatus};
use crate::triage::{self, TriageDecision};
use crate::workflow::{self, StageKind};

/// Process a single issue through the full pipeline using the new config model.
pub async fn process_issue_with_config(
    number: u64,
    config: &RunnerConfig,
    state_dir: &Path,
    repo_dir: &Path,
) -> Result<RunRecord> {
    let repo = &config.global.repo;
    info!(repo = repo, issue = number, "processing issue");

    // 1. Fetch the issue.
    let issue = github::fetch_issue(repo, number).await?;
    info!(
        issue = number,
        title = issue.title,
        labels = ?issue.labels,
        "fetched issue"
    );

    // 2. Match route.
    let (route_name, route) = config
        .match_route(&issue)
        .ok_or_else(|| Error::Triage("no route matches this issue's labels".into()))?;
    info!(issue = number, route = route_name, "matched route");

    // 3. Acquire lease.
    let _ = github::add_label(repo, number, &config.global.in_progress_label).await;
    if let Some(ref gate) = config.global.gate_label {
        let _ = github::remove_label(repo, number, gate).await;
    }

    // 4. Resolve workflow template.
    let template = config
        .resolve_workflow(&route.workflow)
        .ok_or_else(|| Error::Policy(format!("unknown workflow: {}", route.workflow)))?;
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
        None,
        Some((config, repo_dir)),
        &run_id,
    );

    // 6. Set up isolation.
    let worktree_dir = isolation::create_worktree(repo_dir, &branch, ".worktrees").await?;
    info!(issue = number, worktree = %worktree_dir.display(), "created worktree");

    // 7. Execute stages.
    let validation = config.effective_validation(route);

    let mut record = RunRecord::new(&run_id, repo, number, &template.name, &branch);
    let mut all_succeeded = true;
    let mut pending_breadcrumb: Option<String> = None;

    for (stage_idx, planned_stage) in plan.stages.iter().enumerate() {
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
                .current_dir(&worktree_dir)
                .output()
                .await
            && o.status.success()
        {
            info!(
                issue = number,
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
            run_stage_hooks(&h.pre, &worktree_dir, "pre").await;
        }

        if planned_stage.kind == StageKind::Merge && !config.global.auto_merge {
            info!(
                issue = number,
                "skipping merge stage: auto_merge is disabled"
            );
            continue;
        }

        if planned_stage.kind == StageKind::OpenPr {
            let open_pr_success =
                match handle_open_pr(repo, &branch, &issue.title, number, &worktree_dir).await {
                    Ok(pr) => {
                        record.pr_number = Some(pr.number);
                        info!(
                            issue = number,
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
                        error!(issue = number, error = %e, "failed to create PR");
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
                run_stage_hooks(&h.post, &worktree_dir, "post").await;
            }
            if let Some(h) = stage_hooks
                && !h.finally.is_empty()
            {
                run_stage_hooks(&h.finally, &worktree_dir, "finally").await;
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
            issue = number,
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
                .current_dir(&worktree_dir)
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
                issue = number,
                stage = planned_stage.kind_name(),
                success = success,
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
                run_stage_hooks(&h.post, &worktree_dir, "post").await;
            }
            if let Some(h) = stage_hooks
                && !h.finally.is_empty()
            {
                run_stage_hooks(&h.finally, &worktree_dir, "finally").await;
            }
            if failed {
                all_succeeded = false;
                if !planned_stage.optional {
                    break;
                }
            }
            continue;
        }

        let stage_model = planned_stage
            .model
            .as_deref()
            .or_else(|| config.effective_model(route));
        let stage_skills = config.effective_skills(route, planned_stage.skills.as_deref());
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
            .execute_stage(&stage_for_exec, &worktree_dir)
            .await
        {
            Ok(result) => {
                let status = if result.success {
                    StageStatus::Succeeded
                } else {
                    StageStatus::Failed
                };
                info!(
                    issue = number,
                    stage = planned_stage.kind_name(),
                    success = result.success,
                    duration = format!("{:.1}s", result.duration_secs),
                    cost = result.cost_usd,
                    "stage complete"
                );
                let failed = !result.success;
                // Load breadcrumb written by this stage for the next stage's context.
                if !failed && stage_idx + 1 < plan.stages.len() {
                    let crumb_path = worktree_dir
                        .join(".forza")
                        .join("breadcrumbs")
                        .join(&run_id)
                        .join(format!("{}.md", planned_stage.kind_name()));
                    if let Ok(content) = std::fs::read_to_string(&crumb_path) {
                        info!(
                            issue = number,
                            stage = planned_stage.kind_name(),
                            "loaded breadcrumb for next stage"
                        );
                        pending_breadcrumb = Some(content);
                    }
                }
                record.record_stage(planned_stage.kind, status, result);
                if !failed
                    && let Some(h) = stage_hooks
                    && !h.post.is_empty()
                {
                    run_stage_hooks(&h.post, &worktree_dir, "post").await;
                }
                if let Some(h) = stage_hooks
                    && !h.finally.is_empty()
                {
                    run_stage_hooks(&h.finally, &worktree_dir, "finally").await;
                }
                if failed {
                    all_succeeded = false;
                    if !planned_stage.optional {
                        break;
                    }
                }
            }
            Err(e) => {
                error!(issue = number, stage = planned_stage.kind_name(), error = %e, "stage error");
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
                    run_stage_hooks(&h.finally, &worktree_dir, "finally").await;
                }
                all_succeeded = false;
                if !planned_stage.optional {
                    break;
                }
            }
        }

        // Run validation.
        if !validation.is_empty() {
            run_validation(validation, &worktree_dir).await;
        }
    }

    // 8. Finish.
    let final_status = if all_succeeded {
        RunStatus::Succeeded
    } else {
        RunStatus::Failed
    };
    record.finish(final_status);
    state::save_run(&record, state_dir)?;
    info!(issue = number, run_id = record.run_id, status = ?final_status, cost = record.total_cost_usd, "run complete");

    // 9. Update lease labels.
    let _ = github::remove_label(repo, number, &config.global.in_progress_label).await;
    if all_succeeded {
        let _ = github::add_label(repo, number, &config.global.complete_label).await;
    } else {
        let _ = github::add_label(repo, number, &config.global.failed_label).await;
    }

    // 10. Cleanup.
    if all_succeeded && let Err(e) = isolation::remove_worktree(repo_dir, &worktree_dir, true).await
    {
        warn!(error = %e, "failed to clean worktree (non-fatal)");
    }

    Ok(record)
}

/// Process all eligible issues for a config using route matching.
pub async fn process_batch_with_config(
    config: &RunnerConfig,
    state_dir: &Path,
    repo_dir: &Path,
    cancel: &tokio::sync::watch::Receiver<bool>,
) -> Vec<RunRecord> {
    let repo = &config.global.repo;

    // Recover stale leases before processing new work.
    let stale_timeout = std::time::Duration::from_secs(config.global.stale_lease_timeout);
    let now = Utc::now();
    let surviving_in_progress: Vec<crate::github::IssueCandidate> =
        match github::fetch_issues_with_label(repo, &config.global.in_progress_label).await {
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
                        let _ = github::remove_label(
                            repo,
                            issue.number,
                            &config.global.in_progress_label,
                        )
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

    let issues = match github::fetch_eligible_issues(repo, &labels, 10).await {
        Ok(issues) => issues,
        Err(e) => {
            error!(error = %e, "failed to fetch issues");
            return vec![];
        }
    };

    info!(repo = repo, count = issues.len(), "found eligible issues");

    // Pre-filter: keep only issues that match a route, paired with their route name.
    let mut pending: VecDeque<(String, crate::github::IssueCandidate)> = issues
        .into_iter()
        .filter_map(|issue| {
            if let Some((route_name, _)) = config.match_route(&issue) {
                Some((route_name.to_string(), issue))
            } else {
                tracing::debug!(issue = issue.number, "no route match, skipping");
                None
            }
        })
        .collect();

    let max_concurrency = config.global.max_concurrency;
    // Seed active counts from non-stale in-progress issues so that limits
    // are respected correctly after a restart.
    let mut total_active: usize = 0;
    let mut active_per_route: HashMap<String, usize> = HashMap::new();
    for issue in &surviving_in_progress {
        total_active += 1;
        if let Some((route_name, _)) = config.match_route(issue) {
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
        let mut deferred: VecDeque<(String, crate::github::IssueCandidate)> = VecDeque::new();
        while let Some((route_name, issue)) = pending.pop_front() {
            let route_active = *active_per_route.get(&route_name).unwrap_or(&0);
            let route_limit = config
                .routes
                .get(&route_name)
                .map(|r| r.concurrency)
                .unwrap_or(1);

            if total_active < max_concurrency && route_active < route_limit {
                total_active += 1;
                *active_per_route.entry(route_name.clone()).or_insert(0) += 1;

                let config_clone = config.clone();
                let state_dir_owned: PathBuf = state_dir.to_path_buf();
                let repo_dir_owned: PathBuf = repo_dir.to_path_buf();
                let issue_number = issue.number;
                join_set.spawn(async move {
                    let result = process_issue_with_config(
                        issue_number,
                        &config_clone,
                        &state_dir_owned,
                        &repo_dir_owned,
                    )
                    .await;
                    (route_name, result)
                });
            } else if total_active >= max_concurrency {
                info!(
                    issue = issue.number,
                    route = route_name,
                    active = total_active,
                    max = max_concurrency,
                    "global concurrency limit reached, deferring issue"
                );
                deferred.push_back((route_name, issue));
            } else {
                info!(
                    issue = issue.number,
                    route = route_name,
                    active = route_active,
                    limit = route_limit,
                    "per-route concurrency limit reached, deferring issue"
                );
                deferred.push_back((route_name, issue));
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

    records
}

/// Process a single issue through the full pipeline (legacy API).
///
/// Returns the run record with per-stage results.
pub async fn process_issue(
    repo: &str,
    number: u64,
    policy: &RepoPolicy,
    state_dir: &Path,
    repo_dir: &Path,
) -> Result<RunRecord> {
    info!(repo = repo, issue = number, "processing issue");

    // 1. Fetch the issue.
    let issue = github::fetch_issue(repo, number).await?;
    info!(
        issue = number,
        title = issue.title,
        labels = ?issue.labels,
        "fetched issue"
    );

    // 2. Acquire lease (add runner:in-progress, remove runner:ready).
    let _ = github::add_label(repo, number, "runner:in-progress").await;
    let _ = github::remove_label(repo, number, "runner:ready").await;

    // 3. Triage.
    let decision = triage::triage(&issue, policy);
    let needs_clarification = match &decision {
        TriageDecision::Ready => {
            info!(issue = number, "issue is ready for automation");
            false
        }
        TriageDecision::NeedsClarification(questions) => {
            warn!(
                issue = number,
                ?questions,
                "issue needs clarification, injecting clarify stage"
            );
            true
        }
        TriageDecision::OutOfScope(reason) => {
            info!(issue = number, reason = reason, "issue out of scope");
            return Err(Error::Triage(format!("out of scope: {reason}")));
        }
        TriageDecision::AlreadyInProgress => {
            info!(issue = number, "issue already in progress");
            return Err(Error::Triage("already in progress".into()));
        }
        TriageDecision::Blocked(reason) => {
            info!(issue = number, reason = reason, "issue blocked");
            return Err(Error::Triage(format!("blocked: {reason}")));
        }
        TriageDecision::Duplicate(other) => {
            info!(issue = number, duplicate_of = other, "duplicate issue");
            return Err(Error::Triage(format!("duplicate of #{other}")));
        }
    };

    // 3. Select workflow and generate branch.
    let mut template = workflow::select_workflow(&issue.labels, policy);

    // Inject Clarify stage before Plan when triage identified gaps.
    if needs_clarification {
        let clarify_stage = workflow::Stage {
            kind: StageKind::Clarify,
            optional: false,
            max_retries: 1,
            timeout_secs: None,
            condition: None,
            agentless: false,
            command: None,
            model: None,
            skills: None,
            mcp_config: None,
        };
        let plan_pos = template
            .stages
            .iter()
            .position(|s| s.kind == StageKind::Plan);
        if let Some(pos) = plan_pos {
            template.stages.insert(pos, clarify_stage);
        } else {
            template.stages.insert(0, clarify_stage);
        }
        info!(
            issue = number,
            workflow = template.name,
            "injected clarify stage before plan"
        );
    }
    let branch = policy.branch_for_issue(&issue);
    info!(
        issue = number,
        workflow = template.name,
        branch = branch,
        stages = template.stages.len(),
        "selected workflow"
    );

    // 4. Create work plan.
    let run_id = state::generate_run_id();
    let plan = planner::create_plan(&issue, &template, &branch, Some(policy), &run_id);

    // 5. Set up isolation.
    let worktree_dir = isolation::create_worktree(repo_dir, &branch, ".worktrees").await?;
    info!(
        issue = number,
        worktree = %worktree_dir.display(),
        "created worktree"
    );

    // 6. Execute stages.
    let adapter = build_adapter(policy);
    let mut record = RunRecord::new(&run_id, repo, number, &template.name, &branch);

    let mut all_succeeded = true;
    let mut pending_breadcrumb: Option<String> = None;

    for (stage_idx, planned_stage) in plan.stages.iter().enumerate() {
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

        // Evaluate condition if set — skip stage if condition exits non-zero.
        if let Some(ref condition) = planned_stage.condition
            && eval_stage_condition(condition, &issue, &worktree_dir).await
        {
            info!(
                issue = number,
                stage = planned_stage.kind_name(),
                condition = condition,
                "stage condition not met, skipping"
            );
            record.record_stage_skipped(planned_stage.kind);
            continue;
        }

        // Handle OpenPr specially — it's a platform operation.
        if planned_stage.kind == StageKind::OpenPr {
            match handle_open_pr(repo, &branch, &issue.title, number, &worktree_dir).await {
                Ok(pr) => {
                    record.pr_number = Some(pr.number);
                    info!(
                        issue = number,
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
                }
                Err(e) => {
                    error!(issue = number, error = %e, "failed to create PR");
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
                    all_succeeded = false;
                    if !planned_stage.optional {
                        break;
                    }
                }
            }
            continue;
        }

        // Execute agent stage.
        info!(
            issue = number,
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
                .current_dir(&worktree_dir)
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
                issue = number,
                stage = planned_stage.kind_name(),
                success = success,
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
            if failed {
                all_succeeded = false;
                if !planned_stage.optional {
                    break;
                }
            }
            continue;
        }

        match adapter.execute_stage(&stage_for_exec, &worktree_dir).await {
            Ok(result) => {
                let status = if result.success {
                    StageStatus::Succeeded
                } else {
                    StageStatus::Failed
                };
                info!(
                    issue = number,
                    stage = planned_stage.kind_name(),
                    success = result.success,
                    duration = format!("{:.1}s", result.duration_secs),
                    cost = result.cost_usd,
                    "stage complete"
                );
                let failed = !result.success;
                // Load breadcrumb written by this stage for the next stage's context.
                if !failed && stage_idx + 1 < plan.stages.len() {
                    let crumb_path = worktree_dir
                        .join(".forza")
                        .join("breadcrumbs")
                        .join(&run_id)
                        .join(format!("{}.md", planned_stage.kind_name()));
                    if let Ok(content) = std::fs::read_to_string(&crumb_path) {
                        info!(
                            issue = number,
                            stage = planned_stage.kind_name(),
                            "loaded breadcrumb for next stage"
                        );
                        pending_breadcrumb = Some(content);
                    }
                }
                record.record_stage(planned_stage.kind, status, result);

                if failed {
                    all_succeeded = false;
                    if !planned_stage.optional {
                        break;
                    }
                }
            }
            Err(e) => {
                error!(
                    issue = number,
                    stage = planned_stage.kind_name(),
                    error = %e,
                    "stage execution error"
                );
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
                all_succeeded = false;
                if !planned_stage.optional {
                    break;
                }
            }
        }

        // Run validation commands between stages if configured.
        if !policy.validation_commands.is_empty() {
            run_validation(&policy.validation_commands, &worktree_dir).await;
        }
    }

    // 7. Finish run.
    let final_status = if all_succeeded {
        RunStatus::Succeeded
    } else {
        RunStatus::Failed
    };
    record.finish(final_status);

    // 8. Save state.
    state::save_run(&record, state_dir)?;
    info!(
        issue = number,
        run_id = record.run_id,
        status = ?final_status,
        cost = record.total_cost_usd,
        "run complete"
    );

    // 9. Update lease labels.
    let _ = github::remove_label(repo, number, "runner:in-progress").await;
    if all_succeeded {
        let _ = github::add_label(repo, number, "runner:complete").await;
    } else {
        let _ = github::add_label(repo, number, "runner:failed").await;
    }

    // 10. Cleanup on success.
    if all_succeeded {
        if let Err(e) = isolation::remove_worktree(repo_dir, &worktree_dir, true).await {
            warn!(error = %e, "failed to clean worktree (non-fatal)");
        }
    } else {
        info!(
            worktree = %worktree_dir.display(),
            "keeping worktree for inspection (run failed)"
        );
    }

    Ok(record)
}

async fn handle_open_pr(
    repo: &str,
    branch: &str,
    issue_title: &str,
    issue_number: u64,
    work_dir: &Path,
) -> Result<github::PullRequest> {
    // Commit any uncommitted changes (tracked files only — skip breadcrumbs/temp files).
    let status = tokio::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(work_dir)
        .output()
        .await?;
    let has_changes = !String::from_utf8_lossy(&status.stdout).trim().is_empty();

    if has_changes {
        let _ = tokio::process::Command::new("git")
            .args(["add", "-u"])
            .current_dir(work_dir)
            .output()
            .await;
        let _ = tokio::process::Command::new("git")
            .args([
                "commit",
                "-m",
                &format!("automation: implement changes for #{issue_number}"),
            ])
            .current_dir(work_dir)
            .output()
            .await;
    }

    // Rebase on latest origin/main to avoid conflicts with recently merged PRs.
    let _ = tokio::process::Command::new("git")
        .args(["fetch", "origin"])
        .current_dir(work_dir)
        .output()
        .await;
    let rebase = tokio::process::Command::new("git")
        .args(["rebase", "origin/main"])
        .current_dir(work_dir)
        .output()
        .await;
    if let Ok(ref o) = rebase
        && !o.status.success()
    {
        // Rebase failed — abort and try pushing anyway.
        let _ = tokio::process::Command::new("git")
            .args(["rebase", "--abort"])
            .current_dir(work_dir)
            .output()
            .await;
        tracing::warn!(
            issue = issue_number,
            "rebase on origin/main failed, pushing as-is"
        );
    }

    // Push (force to handle stale remote branches from previous failed runs).
    github::push_branch_force(work_dir, branch).await?;

    // Create PR.
    let body = format!(
        "## Summary\n\n\
         Automated implementation for #{issue_number}.\n\n\
         ## Test plan\n\n\
         - [ ] CI passes\n\
         - [ ] Manual review\n\n\
         Closes #{issue_number}"
    );

    github::create_pull_request(repo, branch, issue_title, &body, work_dir).await
}

/// Evaluate a stage condition shell command. Returns `true` if the stage should be skipped.
///
/// The following env vars are available to the condition script:
/// - `RUNNER_ISSUE_NUMBER`
/// - `RUNNER_ISSUE_TITLE`
/// - `RUNNER_ISSUE_BODY`
/// - `RUNNER_ISSUE_LABELS` (comma-separated)
async fn eval_stage_condition(
    condition: &str,
    issue: &github::IssueCandidate,
    work_dir: &Path,
) -> bool {
    let output = tokio::process::Command::new("sh")
        .args(["-c", condition])
        .current_dir(work_dir)
        .env("RUNNER_ISSUE_NUMBER", issue.number.to_string())
        .env("RUNNER_ISSUE_TITLE", &issue.title)
        .env("RUNNER_ISSUE_BODY", &issue.body)
        .env("RUNNER_ISSUE_LABELS", issue.labels.join(","))
        .output()
        .await;

    match output {
        Ok(o) => !o.status.success(),
        Err(e) => {
            warn!(
                condition = condition,
                error = %e,
                "condition command failed to run; not skipping stage"
            );
            false
        }
    }
}

fn build_adapter(policy: &RepoPolicy) -> ClaudeAdapter {
    let mut adapter = ClaudeAdapter::new();
    if let Some(ref model) = policy.model {
        adapter = adapter.model(model);
    }
    adapter
}

async fn run_stage_hooks(hooks: &[String], work_dir: &Path, label: &str) {
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

async fn run_validation(commands: &[String], work_dir: &Path) {
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

/// Process multiple issues from a repo poll.
///
/// Respects both the global `max_concurrency` limit and per-workflow limits
/// from the `concurrency` map. Active runs are identified by the
/// `runner:in-progress` label on GitHub issues.
pub async fn process_batch(
    repo: &str,
    policy: &RepoPolicy,
    state_dir: &Path,
    repo_dir: &Path,
) -> Vec<RunRecord> {
    let labels = if policy.eligible_labels.is_empty() {
        vec![]
    } else {
        policy.eligible_labels.clone()
    };

    let issues = match github::fetch_eligible_issues(repo, &labels, 10).await {
        Ok(issues) => issues,
        Err(e) => {
            error!(error = %e, "failed to fetch issues");
            return vec![];
        }
    };

    info!(repo = repo, count = issues.len(), "found eligible issues");

    // Fetch currently active runs via the runner:in-progress lease label.
    let active_issues = match github::fetch_issues_with_label(repo, "runner:in-progress").await {
        Ok(issues) => issues,
        Err(e) => {
            warn!(error = %e, "failed to fetch in-progress issues, skipping batch");
            return vec![];
        }
    };

    let mut global_active = active_issues.len();
    info!(
        repo = repo,
        active = global_active,
        max = policy.max_concurrency,
        "concurrency check"
    );

    // Count active runs per workflow type.
    let mut workflow_active: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for active in &active_issues {
        let wf = workflow::select_workflow(&active.labels, policy);
        *workflow_active.entry(wf.name).or_insert(0) += 1;
    }

    let mut records = Vec::new();
    for issue in issues {
        // Check global concurrency limit.
        if global_active >= policy.max_concurrency {
            info!(
                active = global_active,
                max = policy.max_concurrency,
                "global concurrency limit reached, stopping batch"
            );
            break;
        }

        // Check per-workflow concurrency limit.
        let wf = workflow::select_workflow(&issue.labels, policy);
        if let Some(&limit) = policy.concurrency.get(&wf.name)
            && limit > 0
        {
            let wf_count = workflow_active.get(&wf.name).copied().unwrap_or(0);
            if wf_count >= limit {
                info!(
                    issue = issue.number,
                    workflow = wf.name,
                    active = wf_count,
                    limit = limit,
                    "per-workflow concurrency limit reached, skipping issue"
                );
                continue;
            }
        }

        // Update local tracking before we start (optimistic — avoids
        // starting two issues of the same workflow in the same batch).
        global_active += 1;
        *workflow_active.entry(wf.name.clone()).or_insert(0) += 1;

        match process_issue(repo, issue.number, policy, state_dir, repo_dir).await {
            Ok(record) => records.push(record),
            Err(e) => {
                warn!(issue = issue.number, error = %e, "failed to process issue");
                // Roll back the optimistic increment on error (the lease
                // may not have been acquired).
                global_active = global_active.saturating_sub(1);
                if let Some(c) = workflow_active.get_mut(&wf.name) {
                    *c = c.saturating_sub(1);
                }
            }
        }
    }

    records
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
}
