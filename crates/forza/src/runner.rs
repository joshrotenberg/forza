//! Runner — discovery, scheduling, and pipeline execution.
//!
//! Replaces the old orchestrator module with a cleaner architecture:
//! - Discovery: fetch eligible subjects from GitHub
//! - Matching: bind subjects to routes (once, carried through)
//! - Scheduling: concurrency management via JoinSet
//! - Execution: delegates to forza_core::pipeline::execute
//!
//! No reactive dispatch. No re-matching. One path for issues and PRs.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::Arc;

use chrono::Utc;
use indexmap::IndexMap;
use tokio::task::JoinSet;
use tracing::{debug, error, info, warn};

use forza_core::lifecycle::LifecycleLabels;
use forza_core::pipeline::{self, PipelineConfig, StageHooks};
use forza_core::planner;
use forza_core::route::MatchedWork;
use forza_core::run::Run;
use forza_core::stage::Workflow;
use forza_core::subject::SubjectKind;
use forza_core::RouteCondition;

use crate::adapters::{self, AgentAdapter, GitAdapter, GitHubAdapter};
use crate::config::{self, Route, RunnerConfig};
use crate::state;

// ── Discovery ───────────────────────────────────────────────────────────

/// Discover all eligible work for a repo and bind each to its route.
async fn discover(
    repo: &str,
    config: &RunnerConfig,
    routes: &IndexMap<String, Route>,
    state_dir: &Path,
    gh: &dyn crate::github::GitHubClient,
) -> Vec<MatchedWork> {
    let mut work = Vec::new();
    let branch_prefix = config
        .global
        .branch_pattern
        .split('{')
        .next()
        .unwrap_or("automation/");

    // 1. Discover issues with gate label.
    let labels = config
        .global
        .gate_label
        .as_deref()
        .map(|l| vec![l.to_string()])
        .unwrap_or_default();

    match gh.fetch_eligible_issues(repo, &labels, 10).await {
        Ok(issues) => {
            info!(repo, count = issues.len(), "found eligible issues");
            for issue in &issues {
                if let Some((route_name, route)) = RunnerConfig::match_route_in(routes, issue) {
                    let wf_name = route.workflow.as_deref().unwrap_or("");
                    if config.resolve_workflow(wf_name).is_some() {
                        let branch = generate_branch(&config.global.branch_pattern, issue.number, &issue.title);
                        let subject = adapters::issue_to_subject(issue, &branch);
                        work.push(MatchedWork {
                            subject,
                            route_name: route_name.to_string(),
                            route: to_core_route(route),
                            workflow_name: wf_name.to_string(),
                        });
                    }
                } else {
                    warn!(
                        issue = issue.number,
                        labels = ?issue.labels,
                        "no route matches issue labels, skipping"
                    );
                }
            }
        }
        Err(e) => error!(error = %e, "failed to fetch issues"),
    }

    // 2. Discover PRs for label routes.
    let label_pr_routes: Vec<(&str, &Route)> = routes
        .iter()
        .filter(|(_, r)| {
            r.route_type == config::SubjectType::Pr
                && r.label.is_some()
                && r.condition.is_none()
        })
        .map(|(name, route)| (name.as_str(), route))
        .collect();

    for (route_name, route) in &label_pr_routes {
        let label = route.label.as_deref().unwrap();
        match gh.fetch_prs_with_label(repo, label).await {
            Ok(prs) => {
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
                info!(repo, route = route_name, count = actionable.len(), "found eligible PRs");
                for pr in &actionable {
                    let wf_name = route.workflow.as_deref().unwrap_or("");
                    if config.resolve_workflow(wf_name).is_some() {
                        let subject = adapters::pr_to_subject(pr);
                        work.push(MatchedWork {
                            subject,
                            route_name: route_name.to_string(),
                            route: to_core_route(route),
                            workflow_name: wf_name.to_string(),
                        });
                    }
                }
            }
            Err(e) => warn!(route = route_name, error = %e, "failed to fetch PRs for route"),
        }
    }

    // 3. Discover PRs for condition routes.
    let condition_routes: Vec<(&str, &Route)> = routes
        .iter()
        .filter(|(_, r)| r.route_type == config::SubjectType::Pr && r.condition.is_some())
        .map(|(name, route)| (name.as_str(), route))
        .collect();

    if !condition_routes.is_empty() {
        let all_prs = match gh.fetch_all_open_prs(repo, 100).await {
            Ok(prs) => {
                info!(repo, count = prs.len(), "fetched open PRs for condition route evaluation");
                prs
            }
            Err(e) => {
                warn!(error = %e, "failed to fetch open PRs for condition routes");
                vec![]
            }
        };

        for (route_name, route) in &condition_routes {
            let condition = route.condition.as_ref().unwrap();
            let mut matched = 0usize;
            for pr in &all_prs {
                // Skip blocked PRs.
                if pr.labels.iter().any(|l| {
                    l == &config.global.in_progress_label || l == "forza:needs-human"
                }) {
                    debug!(pr = pr.number, route = route_name, "skipping: in-progress or needs-human");
                    continue;
                }

                // Scope filter.
                if route.scope == config::ConditionScope::ForzaOwned
                    && !pr.head_branch.starts_with(branch_prefix)
                {
                    debug!(pr = pr.number, route = route_name, "skipping: outside forza_owned scope");
                    continue;
                }

                // Condition check (uses forza-core's RouteCondition via the old type).
                let subject = adapters::pr_to_subject(pr);
                let core_condition = to_core_condition(condition);
                if !core_condition.matches(&subject) {
                    debug!(pr = pr.number, route = route_name, "skipping: condition not matched");
                    continue;
                }

                // Retry budget + exponential backoff.
                let wf_name = route.workflow.as_deref().unwrap_or("");
                let prior_fails = state::count_failed_runs_for_subject(pr.number, wf_name, state_dir);
                if let Some(max) = route.max_retries
                    && prior_fails >= max
                {
                    warn!(pr = pr.number, route = route_name, prior_fails, max, "retry budget exhausted");
                    let _ = gh.add_pr_label(repo, pr.number, "forza:needs-human").await;
                    let _ = gh.comment_on_pr(repo, pr.number, &format!(
                        "Retry budget exhausted for route `{route_name}` ({prior_fails}/{max} attempts). \
                         Applying `forza:needs-human` for manual review."
                    )).await;
                    continue;
                }

                if prior_fails > 0
                    && let Some(last) = state::find_last_completed_run_for_subject(pr.number, wf_name, state_dir)
                    && let Some(completed_at) = last.completed_at
                {
                    let exponent = (prior_fails - 1).min(6) as u32;
                    let backoff_secs = route.poll_interval * 2u64.pow(exponent);
                    let elapsed = Utc::now().signed_duration_since(completed_at).num_seconds().max(0) as u64;
                    if elapsed < backoff_secs {
                        debug!(pr = pr.number, route = route_name, prior_fails, elapsed, backoff_secs, "within backoff window");
                        continue;
                    }
                }

                info!(pr = pr.number, route = route_name, condition = ?condition, "condition matched, queuing PR");
                work.push(MatchedWork {
                    subject,
                    route_name: route_name.to_string(),
                    route: to_core_route(route),
                    workflow_name: wf_name.to_string(),
                });
                matched += 1;
            }
            info!(repo, route = route_name, count = matched, "found eligible PRs for condition route");
        }
    }

    work
}

// ── Batch processing ────────────────────────────────────────────────────

/// Process all eligible work for a repo in one batch cycle.
///
/// Replaces `process_batch_for_repo` from the old orchestrator.
#[allow(clippy::too_many_arguments)]
pub async fn process_batch(
    repo: &str,
    config: &RunnerConfig,
    state_dir: &Path,
    repo_dir: &Path,
    routes: &IndexMap<String, Route>,
    cancel: &tokio::sync::watch::Receiver<bool>,
    gh: Arc<dyn crate::github::GitHubClient>,
    git: Arc<dyn crate::git::GitClient>,
) -> Vec<Run> {
    // Recover stale leases.
    recover_stale_leases(repo, config, &*gh).await;

    // Discover work.
    let candidates = discover(repo, config, routes, state_dir, &*gh).await;

    // Schedule and execute.
    let gh_adapter = Arc::new(GitHubAdapter::new(gh.clone()));
    let git_adapter = Arc::new(GitAdapter::new(git.clone()));
    let agent = Arc::new(AgentAdapter);

    let max_concurrency = config.global.max_concurrency;
    let mut pending: VecDeque<MatchedWork> = candidates.into();
    let mut total_active = 0usize;
    let mut active_per_route: HashMap<String, usize> = HashMap::new();
    let mut join_set: JoinSet<(String, Run)> = JoinSet::new();
    let mut results = Vec::new();

    loop {
        if *cancel.borrow() {
            info!("cancellation requested, stopping batch");
            break;
        }

        // Fill available slots.
        let mut deferred: VecDeque<MatchedWork> = VecDeque::new();
        while let Some(work) = pending.pop_front() {
            let route_active = *active_per_route.get(&work.route_name).unwrap_or(&0);
            let route_limit = work.route.concurrency;

            if total_active < max_concurrency && route_active < route_limit {
                total_active += 1;
                *active_per_route.entry(work.route_name.clone()).or_insert(0) += 1;

                let config_clone = config.clone();
                let state_dir_owned = state_dir.to_path_buf();
                let repo_dir_owned = repo_dir.to_path_buf();
                let gh_adapter_clone = gh_adapter.clone();
                let git_adapter_clone = git_adapter.clone();
                let agent_clone = agent.clone();
                let route_name = work.route_name.clone();

                join_set.spawn(async move {
                    let run = execute_work(
                        work,
                        &config_clone,
                        &state_dir_owned,
                        &repo_dir_owned,
                        &*gh_adapter_clone,
                        &*git_adapter_clone,
                        &*agent_clone,
                    )
                    .await;
                    (route_name, run)
                });
            } else {
                deferred.push_back(work);
            }
        }
        pending = deferred;

        if pending.is_empty() && join_set.is_empty() {
            break;
        }

        // Wait for one task to finish.
        if let Some(join_result) = join_set.join_next().await {
            match join_result {
                Ok((route_name, run)) => {
                    total_active = total_active.saturating_sub(1);
                    if let Some(c) = active_per_route.get_mut(&route_name) {
                        *c = c.saturating_sub(1);
                    }
                    results.push(run);
                }
                Err(e) => {
                    total_active = total_active.saturating_sub(1);
                    warn!(error = %e, "task join error");
                }
            }
        }
    }

    // Drain remaining tasks.
    while let Some(join_result) = join_set.join_next().await {
        match join_result {
            Ok((_, run)) => results.push(run),
            Err(e) => warn!(error = %e, "task join error"),
        }
    }

    results
}

// ── Single work item execution ──────────────────────────────────────────

/// Execute a single matched work item through the forza-core pipeline.
async fn execute_work(
    work: MatchedWork,
    config: &RunnerConfig,
    state_dir: &Path,
    repo_dir: &Path,
    gh: &dyn forza_core::GitHubClient,
    git: &dyn forza_core::GitClient,
    agent: &dyn forza_core::AgentExecutor,
) -> Run {
    // Resolve the workflow.
    let workflow = match resolve_workflow(config, &work.workflow_name) {
        Some(wf) => wf,
        None => {
            error!(
                workflow = work.workflow_name,
                "unknown workflow, cannot execute"
            );
            let mut run = forza_core::Run::new(
                forza_core::generate_run_id(),
                &work.subject.repo,
                work.subject.number,
                work.subject.kind,
                &work.route_name,
                &work.workflow_name,
                &work.subject.branch,
            );
            run.finish(forza_core::RunStatus::Failed);
            run.outcome = Some(forza_core::Outcome::Failed {
                stage: "setup".into(),
                error: format!("unknown workflow: {}", work.workflow_name),
            });
            return run;
        }
    };

    // Build pipeline config from global + route overrides.
    let pipeline_config = build_pipeline_config(config, &work);

    // Generate prompts.
    let preamble = planner::make_preamble(&work.subject.repo);
    let prompts = planner::generate_prompts(
        &work.subject,
        &workflow,
        "pending", // run_id isn't known yet; breadcrumb paths use the actual run_id from pipeline
        &pipeline_config.validation,
        &preamble,
    );

    // Execute.
    pipeline::execute(
        &work,
        &workflow,
        &pipeline_config,
        state_dir,
        repo_dir,
        gh,
        git,
        agent,
        &prompts,
    )
    .await
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Resolve a workflow name to a forza-core Workflow.
fn resolve_workflow(config: &RunnerConfig, name: &str) -> Option<Workflow> {
    let old_template = config.resolve_workflow(name)?;
    Some(Workflow {
        name: old_template.name.clone(),
        stages: old_template
            .stages
            .iter()
            .map(|s| {
                let kind = to_core_stage_kind(s.kind);
                if s.agentless {
                    let cmd = s.command.clone().unwrap_or_default();
                    let mut stage = forza_core::Stage::shell(kind, cmd);
                    stage.optional = s.optional;
                    stage.condition = s.condition.clone();
                    stage.model = s.model.clone();
                    stage.skills = s.skills.clone();
                    stage.mcp_config = s.mcp_config.clone();
                    stage
                } else {
                    let mut stage = forza_core::Stage::agent(kind);
                    stage.optional = s.optional;
                    stage.condition = s.condition.clone();
                    stage.model = s.model.clone();
                    stage.skills = s.skills.clone();
                    stage.mcp_config = s.mcp_config.clone();
                    stage
                }
            })
            .collect(),
        needs_worktree: !old_template.stages.iter().all(|s| s.agentless),
    })
}

/// Build a PipelineConfig from the runner config + route overrides.
fn build_pipeline_config(config: &RunnerConfig, work: &MatchedWork) -> PipelineConfig {
    let validation = config
        .validation
        .commands
        .clone();

    let labels = LifecycleLabels {
        in_progress: config.global.in_progress_label.clone(),
        complete: config.global.complete_label.clone(),
        failed: config.global.failed_label.clone(),
        needs_human: "forza:needs-human".into(),
        gate: config.global.gate_label.clone(),
    };

    // Resolve model: route > global.
    let model = work.route.model.clone()
        .or_else(|| config.global.model.clone());

    // Resolve skills: route > global.
    let skills = work.route.skills.clone()
        .unwrap_or_else(|| config.agent_config.skills.clone());

    let mcp_config = work.route.mcp_config.clone()
        .or_else(|| config.agent_config.mcp_config.clone());

    let append_system_prompt = config.agent_config.append_system_prompt.clone();

    // Build stage hooks.
    let mut stage_hooks = HashMap::new();
    for (stage_name, hooks) in &config.stage_hooks {
        stage_hooks.insert(
            stage_name.clone(),
            StageHooks {
                pre: hooks.pre.clone(),
                post: hooks.post.clone(),
                finally: hooks.finally.clone(),
            },
        );
    }

    PipelineConfig {
        labels,
        model,
        skills,
        mcp_config,
        validation,
        append_system_prompt,
        stage_hooks,
    }
}

/// Convert old config Route to forza-core Route.
fn to_core_route(route: &Route) -> forza_core::Route {
    let trigger = if let Some(ref label) = route.label {
        forza_core::Trigger::Label(label.clone())
    } else if let Some(ref cond) = route.condition {
        forza_core::Trigger::Condition(to_core_condition(cond))
    } else {
        forza_core::Trigger::Label(String::new())
    };

    let scope = match route.scope {
        config::ConditionScope::ForzaOwned => forza_core::Scope::ForzaOwned,
        config::ConditionScope::All => forza_core::Scope::All,
    };

    forza_core::Route {
        subject_type: match route.route_type {
            config::SubjectType::Issue => SubjectKind::Issue,
            config::SubjectType::Pr => SubjectKind::Pr,
        },
        trigger,
        workflow: route.workflow.clone().unwrap_or_default(),
        scope,
        concurrency: route.concurrency,
        poll_interval: route.poll_interval,
        max_retries: route.max_retries,
        model: route.model.clone(),
        skills: route.skills.clone(),
        mcp_config: route.mcp_config.clone(),
        validation_commands: route.validation_commands.clone(),
    }
}

/// Convert old RouteCondition to forza-core RouteCondition.
fn to_core_condition(cond: &config::RouteCondition) -> RouteCondition {
    match cond {
        config::RouteCondition::CiFailing => RouteCondition::CiFailing,
        config::RouteCondition::HasConflicts => RouteCondition::HasConflicts,
        config::RouteCondition::CiFailingOrConflicts => RouteCondition::CiFailingOrConflicts,
        config::RouteCondition::ApprovedAndGreen => RouteCondition::ApprovedAndGreen,
        config::RouteCondition::CiGreenNoObjections => RouteCondition::CiGreenNoObjections,
        config::RouteCondition::AnyActionable => RouteCondition::CiGreenNoObjections, // fallback
    }
}

/// Convert old StageKind to forza-core StageKind.
fn to_core_stage_kind(kind: crate::workflow::StageKind) -> forza_core::StageKind {
    match kind {
        crate::workflow::StageKind::Triage => forza_core::StageKind::Triage,
        crate::workflow::StageKind::Clarify => forza_core::StageKind::Clarify,
        crate::workflow::StageKind::Plan => forza_core::StageKind::Plan,
        crate::workflow::StageKind::Implement => forza_core::StageKind::Implement,
        crate::workflow::StageKind::Test => forza_core::StageKind::Test,
        crate::workflow::StageKind::Review => forza_core::StageKind::Review,
        crate::workflow::StageKind::OpenPr => forza_core::StageKind::OpenPr,
        crate::workflow::StageKind::RevisePr => forza_core::StageKind::RevisePr,
        crate::workflow::StageKind::FixCi => forza_core::StageKind::FixCi,
        crate::workflow::StageKind::Merge => forza_core::StageKind::Merge,
        crate::workflow::StageKind::Research => forza_core::StageKind::Research,
        crate::workflow::StageKind::Comment => forza_core::StageKind::Comment,
    }
}

/// Generate a branch name from the pattern.
fn generate_branch(pattern: &str, number: u64, title: &str) -> String {
    let slug: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let slug = if slug.len() > 40 {
        slug[..40].trim_end_matches('-').to_string()
    } else {
        slug
    };
    pattern
        .replace("{issue}", &number.to_string())
        .replace("{slug}", &slug)
}

/// Recover stale in-progress leases.
async fn recover_stale_leases(
    repo: &str,
    config: &RunnerConfig,
    gh: &dyn crate::github::GitHubClient,
) {
    let stale_timeout = std::time::Duration::from_secs(config.global.stale_lease_timeout);
    let now = Utc::now();
    match gh
        .fetch_issues_with_label(repo, &config.global.in_progress_label)
        .await
    {
        Ok(in_progress) => {
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
                }
            }
        }
        Err(e) => {
            warn!(error = %e, "failed to fetch in-progress issues for stale lease check");
        }
    }
}

// ── Single-item processing (CLI commands) ───────────────────────────────

/// Process a single issue by number. Used by `forza issue`.
#[allow(clippy::too_many_arguments)]
pub async fn process_issue(
    number: u64,
    repo: &str,
    config: &RunnerConfig,
    routes: &IndexMap<String, Route>,
    state_dir: &Path,
    repo_dir: &Path,
    gh: Arc<dyn crate::github::GitHubClient>,
    git: Arc<dyn crate::git::GitClient>,
    model_override: Option<String>,
    skill_overrides: Vec<String>,
) -> forza_core::Result<Run> {
    let issue = gh
        .fetch_issue(repo, number)
        .await
        .map_err(|e| forza_core::Error::GitHub(e.to_string()))?;

    let (route_name, route) = RunnerConfig::match_route_in(routes, &issue)
        .ok_or_else(|| forza_core::Error::NoMatchingRoute(format!(
            "no route matches issue #{number} (labels: {:?})", issue.labels
        )))?;

    let wf_name = route.workflow.as_deref().unwrap_or("");
    let branch = generate_branch(&config.global.branch_pattern, number, &issue.title);
    let subject = adapters::issue_to_subject(&issue, &branch);

    let mut matched = MatchedWork {
        subject,
        route_name: route_name.to_string(),
        route: to_core_route(route),
        workflow_name: wf_name.to_string(),
    };

    // Apply CLI overrides.
    if let Some(m) = model_override {
        matched.route.model = Some(m);
    }
    if !skill_overrides.is_empty() {
        matched.route.skills = Some(skill_overrides);
    }

    let gh_adapter = Arc::new(GitHubAdapter::new(gh));
    let git_adapter = Arc::new(GitAdapter::new(git));
    let agent = AgentAdapter;

    Ok(execute_work(
        matched, config, state_dir, repo_dir, &*gh_adapter, &*git_adapter, &agent,
    ).await)
}

/// Process a single PR by number. Used by `forza pr`.
#[allow(clippy::too_many_arguments)]
pub async fn process_pr(
    number: u64,
    repo: &str,
    config: &RunnerConfig,
    routes: &IndexMap<String, Route>,
    state_dir: &Path,
    repo_dir: &Path,
    gh: Arc<dyn crate::github::GitHubClient>,
    git: Arc<dyn crate::git::GitClient>,
    model_override: Option<String>,
    skill_overrides: Vec<String>,
    route_override: Option<String>,
) -> forza_core::Result<Run> {
    let pr = gh
        .fetch_pr(repo, number)
        .await
        .map_err(|e| forza_core::Error::GitHub(e.to_string()))?;

    // Use route override if provided (from condition routes), otherwise match by labels.
    let (route_name, route) = if let Some(ref rn) = route_override
        && let Some(r) = routes.get(rn)
    {
        (rn.as_str(), r)
    } else {
        RunnerConfig::match_pr_route_in(routes, &pr)
            .ok_or_else(|| forza_core::Error::NoMatchingRoute(format!(
                "no route matches PR #{number} (labels: {:?})", pr.labels
            )))?
    };

    let wf_name = route.workflow.as_deref().unwrap_or("");
    let subject = adapters::pr_to_subject(&pr);

    let mut matched = MatchedWork {
        subject,
        route_name: route_name.to_string(),
        route: to_core_route(route),
        workflow_name: wf_name.to_string(),
    };

    if let Some(m) = model_override {
        matched.route.model = Some(m);
    }
    if !skill_overrides.is_empty() {
        matched.route.skills = Some(skill_overrides);
    }

    let gh_adapter = Arc::new(GitHubAdapter::new(gh));
    let git_adapter = Arc::new(GitAdapter::new(git));
    let agent = AgentAdapter;

    Ok(execute_work(
        matched, config, state_dir, repo_dir, &*gh_adapter, &*git_adapter, &agent,
    ).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_branch_basic() {
        let branch = generate_branch("automation/{issue}-{slug}", 42, "Fix the bug");
        assert_eq!(branch, "automation/42-fix-the-bug");
    }

    #[test]
    fn generate_branch_truncates_long_titles() {
        let branch = generate_branch(
            "automation/{issue}-{slug}",
            123,
            "This is a very long issue title that should be truncated to forty chars",
        );
        assert!(branch.len() < 80);
        assert!(branch.starts_with("automation/123-"));
    }

    #[test]
    fn generate_branch_handles_special_chars() {
        let branch = generate_branch("automation/{issue}-{slug}", 1, "fix: the bug (part 2)");
        assert_eq!(branch, "automation/1-fix-the-bug-part-2");
    }
}
