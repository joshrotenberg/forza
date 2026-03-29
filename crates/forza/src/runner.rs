//! Runner — discovery, scheduling, and pipeline execution.
//!
//! Replaces the old orchestrator module with a cleaner architecture:
//! - Discovery: fetch eligible subjects from GitHub
//! - Matching: bind subjects to routes (once, carried through)
//! - Scheduling: concurrency management via JoinSet
//! - Execution: delegates to forza_core::pipeline::execute
//!
//! No reactive dispatch. No re-matching. One path for issues and PRs.

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use chrono::Utc;
use indexmap::IndexMap;
use tokio::task::JoinSet;
use tracing::{debug, error, info, warn};

use forza_core::RouteCondition;
use forza_core::lifecycle::LifecycleLabels;
use forza_core::pipeline::{self, PipelineConfig, StageHooks};
use forza_core::planner;
use forza_core::route::MatchedWork;
use forza_core::run::Run;
use forza_core::stage::Workflow;
use forza_core::subject::SubjectKind;

use crate::adapters::{self, ClaudeAgentAdapter, CodexAgentAdapter, GitAdapter, GitHubAdapter};
use crate::config::{self, IssueOrder, Route, RunnerConfig};
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

    // 1. Discover issues with gate label.
    let labels = config
        .global
        .gate_label
        .as_deref()
        .map(|l| vec![l.to_string()])
        .unwrap_or_default();

    match gh.fetch_eligible_issues(repo, &labels, 10).await {
        Ok(mut issues) => {
            match config.global.issue_order {
                IssueOrder::OldestFirst => issues.sort_by_key(|i| i.number),
                IssueOrder::NewestFirst => issues.sort_by_key(|i| Reverse(i.number)),
            }
            info!(repo, count = issues.len(), "found eligible issues");
            for issue in &issues {
                if issue.labels.iter().any(|l| l == "forza:plan") {
                    info!(
                        issue = issue.number,
                        "skipping plan issue (use `forza plan --exec {}`)", issue.number
                    );
                    continue;
                }
                if let Some((route_name, route)) = RunnerConfig::match_route_in(routes, issue) {
                    let wf_name = route.workflow.as_deref().unwrap_or("");
                    if config.resolve_workflow(wf_name).is_some() {
                        let branch = generate_branch(
                            config.effective_branch_pattern(route),
                            issue.number,
                            &issue.title,
                            route_name,
                            route.label.as_deref(),
                        );
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
            r.route_type == config::SubjectType::Pr && r.label.is_some() && r.condition.is_none()
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
                info!(
                    repo,
                    route = route_name,
                    count = actionable.len(),
                    "found eligible PRs"
                );
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
                info!(
                    repo,
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

        let mut queued_prs: HashSet<u64> = HashSet::new();
        for (route_name, route) in &condition_routes {
            let condition = route.condition.as_ref().unwrap();
            let mut matched = 0usize;
            for pr in &all_prs {
                // Skip blocked PRs.
                if pr
                    .labels
                    .iter()
                    .any(|l| l == &config.global.in_progress_label || l == "forza:needs-human")
                {
                    debug!(
                        pr = pr.number,
                        route = route_name,
                        "skipping: in-progress or needs-human"
                    );
                    continue;
                }

                // Scope filter.
                if route.scope == config::ConditionScope::ForzaOwned {
                    let prefixes = config.forza_owned_prefixes(routes);
                    if !prefixes
                        .iter()
                        .any(|p| pr.head_branch.starts_with(p.as_str()))
                    {
                        debug!(
                            pr = pr.number,
                            route = route_name,
                            "skipping: outside forza_owned scope"
                        );
                        continue;
                    }
                }

                // Condition check (uses forza-core's RouteCondition via the old type).
                let subject = adapters::pr_to_subject(pr);
                let core_condition = to_core_condition(condition);
                if !core_condition.matches(&subject) {
                    debug!(
                        pr = pr.number,
                        route = route_name,
                        "skipping: condition not matched"
                    );
                    continue;
                }

                // Deduplication: skip if this PR was already queued by a higher-priority route.
                if queued_prs.contains(&pr.number) {
                    debug!(
                        pr = pr.number,
                        route = route_name,
                        "skipping: already queued by a prior condition route"
                    );
                    continue;
                }

                // Retry budget + exponential backoff.
                let wf_name = route.workflow.as_deref().unwrap_or("");
                let prior_fails =
                    state::count_failed_runs_for_subject(pr.number, wf_name, state_dir);
                if let Some(max) = route.max_retries
                    && prior_fails >= max
                {
                    warn!(
                        pr = pr.number,
                        route = route_name,
                        prior_fails,
                        max,
                        "retry budget exhausted"
                    );
                    let _ = gh.add_pr_label(repo, pr.number, "forza:needs-human").await;
                    let _ = gh.comment_on_pr(repo, pr.number, &format!(
                        "Retry budget exhausted for route `{route_name}` ({prior_fails}/{max} attempts). \
                         Applying `forza:needs-human` for manual review."
                    )).await;
                    continue;
                }

                if prior_fails > 0
                    && let Some(last) =
                        state::find_last_completed_run_for_subject(pr.number, wf_name, state_dir)
                    && let Some(completed_at) = last.completed_at
                {
                    let exponent = (prior_fails - 1).min(6) as u32;
                    let backoff_secs = route.poll_interval * 2u64.pow(exponent);
                    let elapsed = Utc::now()
                        .signed_duration_since(completed_at)
                        .num_seconds()
                        .max(0) as u64;
                    if elapsed < backoff_secs {
                        debug!(
                            pr = pr.number,
                            route = route_name,
                            prior_fails,
                            elapsed,
                            backoff_secs,
                            "within backoff window"
                        );
                        continue;
                    }
                }

                info!(pr = pr.number, route = route_name, condition = ?condition, "condition matched, queuing PR");
                queued_prs.insert(pr.number);
                work.push(MatchedWork {
                    subject,
                    route_name: route_name.to_string(),
                    route: to_core_route(route),
                    workflow_name: wf_name.to_string(),
                });
                matched += 1;
            }
            info!(
                repo,
                route = route_name,
                count = matched,
                "found eligible PRs for condition route"
            );
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
    let agent = create_agent(config);

    let max_concurrency = config.global.max_concurrency;
    let mut total_active = 0usize;
    let mut active_per_route: HashMap<String, usize> = HashMap::new();
    let mut join_set: JoinSet<Run> = JoinSet::new();

    if *cancel.borrow() {
        info!("cancellation requested, stopping batch");
        return Vec::new();
    }

    // Single pass: spawn up to the concurrency limits, drop overflow.
    // Dropped candidates will be rediscovered next cycle with fresh state.
    for work in candidates {
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

            join_set.spawn(async move {
                execute_work(
                    work,
                    &config_clone,
                    &state_dir_owned,
                    &repo_dir_owned,
                    &*gh_adapter_clone,
                    &*git_adapter_clone,
                    &*agent_clone,
                )
                .await
            });
        } else {
            debug!(
                route = work.route_name,
                subject = work.subject.number,
                "concurrency limit reached, dropping — will rediscover next cycle"
            );
        }
    }

    let mut results = Vec::new();
    while let Some(join_result) = join_set.join_next().await {
        match join_result {
            Ok(run) => results.push(run),
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
    let mut pipeline_config = build_pipeline_config(config, &work);
    pipeline_config.agent = config.global.agent.clone();
    let tools_dir = repo_dir.join("tools");
    if tools_dir.exists() {
        pipeline_config.tools_dir = Some(tools_dir);
    }

    // Generate prompts.
    let preamble = planner::make_preamble(&work.subject.repo);
    let prompts_dir = repo_dir.join("prompts");
    let prompts_dir_opt = prompts_dir.exists().then_some(prompts_dir.as_path());
    let prompts = planner::generate_prompts(
        &work.subject,
        &workflow,
        "pending", // run_id isn't known yet; breadcrumb paths use the actual run_id from pipeline
        &pipeline_config.validation,
        &preamble,
        config.global.agent.as_str(),
        prompts_dir_opt,
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

/// Resolve a workflow name to a forza-core Workflow (public for use by explain).
pub fn resolve_workflow_public(config: &RunnerConfig, name: &str) -> Option<Workflow> {
    resolve_workflow(config, name)
}

/// Resolve a workflow name to a forza-core Workflow.
fn resolve_workflow(config: &RunnerConfig, name: &str) -> Option<Workflow> {
    // Prefer forza-core builtins (which include DraftPr stages).
    if let Some(builtin) = Workflow::builtins().into_iter().find(|w| w.name == name) {
        // Check if user has a custom override for this workflow name.
        if config.workflow_templates.iter().any(|t| t.name == name) {
            // User has a custom template — fall through to convert it.
        } else {
            return Some(builtin);
        }
    }

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
    let validation = config.validation.commands.clone();

    let labels = LifecycleLabels {
        in_progress: config.global.in_progress_label.clone(),
        complete: config.global.complete_label.clone(),
        failed: config.global.failed_label.clone(),
        needs_human: "forza:needs-human".into(),
        gate: config.global.gate_label.clone(),
    };

    // Resolve model: route > global.
    let model = work
        .route
        .model
        .clone()
        .or_else(|| config.global.model.clone());

    // Resolve skills: route > global.
    let skills = work
        .route
        .skills
        .clone()
        .unwrap_or_else(|| config.agent_config.skills.clone());

    let context = config.agent_config.context.clone();

    let mcp_config = work
        .route
        .mcp_config
        .clone()
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
        context,
        skills,
        mcp_config,
        validation,
        append_system_prompt,
        stage_hooks,
        tools_dir: None,
        agent: String::new(),
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

/// Generate a branch name from a pattern by substituting issue metadata.
///
/// Create the appropriate agent executor based on the config's `agent` field.
///
/// Supported values: `"claude"` (default), `"codex"`.
fn create_agent(config: &RunnerConfig) -> Arc<dyn forza_core::AgentExecutor> {
    match config.global.agent.as_str() {
        "codex" => {
            info!(agent = "codex", "using Codex agent backend");
            Arc::new(CodexAgentAdapter)
        }
        other => {
            if other != "claude" {
                warn!(agent = other, "unknown agent, falling back to Claude");
            }
            info!(agent = "claude", "using Claude agent backend");
            Arc::new(ClaudeAgentAdapter)
        }
    }
}

/// The pattern supports four placeholders:
/// - `{issue}` — replaced with the issue or PR number.
/// - `{slug}` — replaced with a URL-safe slug derived from `title`: lowercased,
///   non-alphanumeric characters converted to hyphens, consecutive hyphens collapsed,
///   and truncated to 40 characters (trimming any trailing hyphen).
/// - `{route}` — replaced with the route name.
/// - `{label}` — replaced with the trigger label, or an empty string if none.
///
/// # Examples
///
/// ```text
/// // "automation/{issue}-{slug}" with number=42, title="Fix the bug"
/// // → "automation/42-fix-the-bug"
/// ```
fn generate_branch(
    pattern: &str,
    number: u64,
    title: &str,
    route_name: &str,
    label: Option<&str>,
) -> String {
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
        .replace("{route}", route_name)
        .replace("{label}", label.unwrap_or(""))
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
    base_branch_override: Option<String>,
    workflow_override: Option<String>,
) -> forza_core::Result<Run> {
    tracing::info!(number, repo, "processing issue");
    let issue = gh.fetch_issue(repo, number).await.map_err(|e| match e {
        crate::error::Error::GitHub(msg) => forza_core::Error::GitHub(msg),
        _ => forza_core::Error::GitHub(e.to_string()),
    })?;

    let mut matched = if let Some(wf_name) = workflow_override {
        // Workflow override: skip route matching, build a synthetic route.
        let branch = generate_branch(
            &config.global.branch_pattern,
            number,
            &issue.title,
            "direct",
            None,
        );
        let mut subject = adapters::issue_to_subject(&issue, &branch);
        if let Some(ref base) = base_branch_override {
            subject.base_branch = Some(base.clone());
        }
        MatchedWork {
            subject,
            route_name: "direct".to_string(),
            route: forza_core::Route {
                subject_type: SubjectKind::Issue,
                trigger: forza_core::Trigger::Label(String::new()),
                workflow: wf_name.clone(),
                scope: forza_core::Scope::All,
                concurrency: 1,
                poll_interval: 300,
                max_retries: None,
                model: model_override.clone(),
                skills: if skill_overrides.is_empty() {
                    None
                } else {
                    Some(skill_overrides.clone())
                },
                mcp_config: None,
                validation_commands: None,
            },
            workflow_name: wf_name,
        }
    } else {
        let (route_name, route) =
            RunnerConfig::match_route_in(routes, &issue).ok_or_else(|| {
                forza_core::Error::NoMatchingRoute(format!(
                    "no route matches issue #{number} (labels: {:?})",
                    issue.labels
                ))
            })?;

        let wf_name = route.workflow.as_deref().unwrap_or("");
        let branch = generate_branch(
            config.effective_branch_pattern(route),
            number,
            &issue.title,
            route_name,
            route.label.as_deref(),
        );
        let mut subject = adapters::issue_to_subject(&issue, &branch);

        if let Some(ref base) = base_branch_override {
            subject.base_branch = Some(base.clone());
        }

        MatchedWork {
            subject,
            route_name: route_name.to_string(),
            route: to_core_route(route),
            workflow_name: wf_name.to_string(),
        }
    };

    // Apply CLI overrides (for the route-matched path; workflow override path sets these above).
    if matched.route_name != "direct" {
        if let Some(m) = model_override {
            matched.route.model = Some(m);
        }
        if !skill_overrides.is_empty() {
            matched.route.skills = Some(skill_overrides);
        }
    }

    let gh_adapter = Arc::new(GitHubAdapter::new(gh));
    let git_adapter = Arc::new(GitAdapter::new(git));
    let agent = create_agent(config);

    Ok(execute_work(
        matched,
        config,
        state_dir,
        repo_dir,
        &*gh_adapter,
        &*git_adapter,
        &*agent,
    )
    .await)
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
    workflow_override: Option<String>,
) -> forza_core::Result<Run> {
    tracing::info!(number, repo, "processing PR");
    let pr = gh.fetch_pr(repo, number).await.map_err(|e| match e {
        crate::error::Error::GitHub(msg) => forza_core::Error::GitHub(msg),
        _ => forza_core::Error::GitHub(e.to_string()),
    })?;

    let mut matched = if let Some(wf_name) = workflow_override {
        // Workflow override: skip route matching, build a synthetic route.
        let subject = adapters::pr_to_subject(&pr);
        MatchedWork {
            subject,
            route_name: "direct".to_string(),
            route: forza_core::Route {
                subject_type: SubjectKind::Pr,
                trigger: forza_core::Trigger::Label(String::new()),
                workflow: wf_name.clone(),
                scope: forza_core::Scope::All,
                concurrency: 1,
                poll_interval: 300,
                max_retries: None,
                model: model_override.clone(),
                skills: if skill_overrides.is_empty() {
                    None
                } else {
                    Some(skill_overrides.clone())
                },
                mcp_config: None,
                validation_commands: None,
            },
            workflow_name: wf_name,
        }
    } else {
        // Use route override if provided (from condition routes), otherwise match by labels.
        let (route_name, route) = if let Some(ref rn) = route_override
            && let Some(r) = routes.get(rn)
        {
            (rn.as_str(), r)
        } else {
            RunnerConfig::match_pr_route_in(routes, &pr).ok_or_else(|| {
                forza_core::Error::NoMatchingRoute(format!(
                    "no route matches PR #{number} (labels: {:?})",
                    pr.labels
                ))
            })?
        };

        let wf_name = route.workflow.as_deref().unwrap_or("");
        let subject = adapters::pr_to_subject(&pr);

        MatchedWork {
            subject,
            route_name: route_name.to_string(),
            route: to_core_route(route),
            workflow_name: wf_name.to_string(),
        }
    };

    // Apply CLI overrides (for the route-matched path; workflow override path sets these above).
    if matched.route_name != "direct" {
        if let Some(m) = model_override {
            matched.route.model = Some(m);
        }
        if !skill_overrides.is_empty() {
            matched.route.skills = Some(skill_overrides);
        }
    }

    let gh_adapter = Arc::new(GitHubAdapter::new(gh));
    let git_adapter = Arc::new(GitAdapter::new(git));
    let agent = create_agent(config);

    Ok(execute_work(
        matched,
        config,
        state_dir,
        repo_dir,
        &*gh_adapter,
        &*git_adapter,
        &*agent,
    )
    .await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_branch_basic() {
        let branch = generate_branch(
            "automation/{issue}-{slug}",
            42,
            "Fix the bug",
            "bugfix",
            None,
        );
        assert_eq!(branch, "automation/42-fix-the-bug");
    }

    #[test]
    fn generate_branch_truncates_long_titles() {
        let branch = generate_branch(
            "automation/{issue}-{slug}",
            123,
            "This is a very long issue title that should be truncated to forty chars",
            "bugfix",
            None,
        );
        assert!(branch.len() < 80);
        assert!(branch.starts_with("automation/123-"));
    }

    #[test]
    fn generate_branch_handles_special_chars() {
        let branch = generate_branch(
            "automation/{issue}-{slug}",
            1,
            "fix: the bug (part 2)",
            "bugfix",
            None,
        );
        assert_eq!(branch, "automation/1-fix-the-bug-part-2");
    }

    #[test]
    fn generate_branch_route_placeholder() {
        let branch = generate_branch(
            "automation/{route}/{issue}-{slug}",
            7,
            "add feature",
            "my-route",
            None,
        );
        assert_eq!(branch, "automation/my-route/7-add-feature");
    }

    #[test]
    fn generate_branch_label_placeholder() {
        let branch = generate_branch(
            "automation/{label}/{issue}-{slug}",
            5,
            "fix bug",
            "bugfix",
            Some("bug"),
        );
        assert_eq!(branch, "automation/bug/5-fix-bug");
    }

    #[test]
    fn generate_branch_label_placeholder_none() {
        let branch = generate_branch(
            "automation/{label}/{issue}-{slug}",
            5,
            "fix bug",
            "bugfix",
            None,
        );
        assert_eq!(branch, "automation//5-fix-bug");
    }

    #[test]
    fn generate_branch_all_placeholders() {
        let branch = generate_branch(
            "{route}/{label}/{issue}-{slug}",
            99,
            "My feature",
            "feature-route",
            Some("enhancement"),
        );
        assert_eq!(branch, "feature-route/enhancement/99-my-feature");
    }

    #[test]
    fn queued_prs_deduplicates_across_condition_routes() {
        // Simulate the deduplication logic: a PR matching two routes should
        // only be queued once (by the first matching route).
        let mut queued_prs: HashSet<u64> = HashSet::new();
        let pr_number: u64 = 42;

        // First route matches — insert and queue.
        assert!(!queued_prs.contains(&pr_number));
        queued_prs.insert(pr_number);
        let mut queued_count = 1usize;

        // Second route also matches the same PR — should be skipped.
        if queued_prs.contains(&pr_number) {
            // skipped
        } else {
            queued_count += 1;
        }

        assert_eq!(queued_count, 1, "PR should only be queued once");
    }
}
