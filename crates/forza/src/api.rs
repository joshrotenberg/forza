//! REST API server — HTTP interface for forza operations.
//!
//! Exposes three endpoint groups:
//! - **Trigger**: `POST /runs/issue/{n}`, `POST /runs/pr/{n}`, `POST /runs/batch`
//! - **Status**: `GET /runs`, `GET /runs/latest`, `GET /runs/{run_id}`, `GET /status`
//! - **Config**: `GET /config`

use std::path::PathBuf;

use indexmap::IndexMap;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::config::{Route, RunnerConfig};
use crate::plan::{
    build_issue_refs, build_issue_summaries, build_route_summary, parse_plan_dag, topological_sort,
};
use crate::state::RunRecord;

/// Shared application state injected into every handler.
pub struct AppState {
    pub config: RunnerConfig,
    pub state_dir: PathBuf,
    pub gh: Arc<dyn crate::github::GitHubClient>,
    pub git: Arc<dyn crate::git::GitClient>,
}

// --- Error type ---

enum ApiError {
    NotFound(String),
    BadRequest(String),
    Internal(String),
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            ApiError::NotFound(m) => (StatusCode::NOT_FOUND, m),
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            ApiError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
        };
        (status, Json(ErrorBody { error: msg })).into_response()
    }
}

// --- Response types ---

/// Returned by trigger endpoints when a background job is queued.
#[derive(Serialize)]
struct AcceptedResponse {
    status: &'static str,
    message: &'static str,
}

/// Returned by a dry-run issue trigger.
#[derive(Serialize)]
struct IssueDryRunResponse {
    issue_number: u64,
    issue_title: String,
    route: String,
    workflow: String,
    branch: String,
    model: Option<String>,
    stages: Vec<String>,
}

/// Returned by a dry-run PR trigger.
#[derive(Serialize)]
struct PrDryRunResponse {
    pr_number: u64,
    pr_title: String,
    route: String,
    workflow: String,
    branch: String,
    model: Option<String>,
    stages: Vec<String>,
}

/// Per-workflow aggregate stats (serializable mirror of `state::WorkflowSummary`).
#[derive(Serialize)]
pub struct WorkflowSummaryResponse {
    workflow: String,
    total_runs: usize,
    succeeded: usize,
    failed: usize,
    min_cost: Option<f64>,
    max_cost: Option<f64>,
    avg_cost: Option<f64>,
}

impl From<crate::state::WorkflowSummary> for WorkflowSummaryResponse {
    fn from(s: crate::state::WorkflowSummary) -> Self {
        Self {
            workflow: s.workflow,
            total_runs: s.total_runs,
            succeeded: s.succeeded,
            failed: s.failed,
            min_cost: s.min_cost,
            max_cost: s.max_cost,
            avg_cost: s.avg_cost,
        }
    }
}

// --- Query params ---

#[derive(Deserialize)]
struct TriggerQuery {
    repo: Option<String>,
    dry_run: Option<bool>,
    /// Override the workflow template, skipping route matching.
    workflow: Option<String>,
    /// Override the model for every stage.
    model: Option<String>,
}

// --- Repo resolution ---

/// Resolve repo, repo_dir, and routes from the config, mirroring the CLI `resolve_repo` logic.
async fn resolve_repo_for_api(
    args_repo: Option<&str>,
    config: &RunnerConfig,
    git: &dyn crate::git::GitClient,
) -> Result<(String, PathBuf, IndexMap<String, Route>), ApiError> {
    let repos = config.iter_repos();
    let (repo_str, entry_repo_dir, routes) = if repos.len() == 1 {
        repos
            .into_iter()
            .next()
            .ok_or_else(|| ApiError::BadRequest("no repos configured".into()))?
    } else {
        match args_repo {
            Some(r) => match repos.into_iter().find(|(repo, _, _)| *repo == r) {
                Some(entry) => entry,
                None => {
                    return Err(ApiError::BadRequest(format!(
                        "repo '{r}' not found in config"
                    )));
                }
            },
            None => {
                return Err(ApiError::BadRequest(
                    "multiple repos configured — use ?repo= to specify which one".into(),
                ));
            }
        }
    };

    let explicit_dir = entry_repo_dir
        .map(PathBuf::from)
        .or_else(|| config.global.repo_dir.as_ref().map(PathBuf::from));

    let rd = crate::isolation::find_or_clone_repo(repo_str, explicit_dir, git)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok((repo_str.to_string(), rd, routes.clone()))
}

// --- Router ---

/// Build the axum router. Call this once and pass the result to `axum::serve`.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        // Status group
        .route("/runs", get(list_runs))
        .route("/runs/latest", get(latest_run))
        .route("/runs/{run_id}", get(get_run))
        .route("/status", get(get_status))
        // Trigger group
        .route("/runs/batch", post(trigger_batch))
        .route("/runs/issue/{number}", post(trigger_issue))
        .route("/runs/pr/{number}", post(trigger_pr))
        // Config group
        .route("/config", get(get_config))
        // Plan group
        .route("/plans", post(create_plan))
        .route("/plans", get(list_plans))
        .route("/plans/{number}", get(get_plan))
        .route("/plans/{number}/revise", post(revise_plan))
        .route("/plans/{number}/exec", post(exec_plan))
        .route("/plans/{number}/exec/status", get(plan_exec_status))
        .with_state(state)
}

// --- Status handlers ---

async fn list_runs(State(state): State<Arc<AppState>>) -> Json<Vec<RunRecord>> {
    Json(crate::state::load_all_runs(&state.state_dir))
}

async fn latest_run(State(state): State<Arc<AppState>>) -> Result<Json<RunRecord>, ApiError> {
    crate::state::load_latest(&state.state_dir)
        .map(Json)
        .ok_or_else(|| ApiError::NotFound("no runs found".into()))
}

async fn get_run(
    Path(run_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<RunRecord>, ApiError> {
    crate::state::load_run(&run_id, &state.state_dir)
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("run not found: {run_id}")))
}

async fn get_status(State(state): State<Arc<AppState>>) -> Json<Vec<WorkflowSummaryResponse>> {
    let summaries = crate::state::summarize_by_workflow(&state.state_dir)
        .into_iter()
        .map(WorkflowSummaryResponse::from)
        .collect();
    Json(summaries)
}

// --- Config handler ---

async fn get_config(State(state): State<Arc<AppState>>) -> Json<RunnerConfig> {
    Json(state.config.clone())
}

// --- Trigger handlers ---

async fn trigger_issue(
    Path(number): Path<u64>,
    Query(query): Query<TriggerQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<Response, ApiError> {
    let (repo, rd, routes) =
        resolve_repo_for_api(query.repo.as_deref(), &state.config, &*state.git).await?;

    if query.dry_run.unwrap_or(false) {
        let issue = state
            .gh
            .fetch_issue(&repo, number)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        let (route_name, route) =
            RunnerConfig::match_route_in(&routes, &issue).ok_or_else(|| {
                ApiError::BadRequest(format!(
                    "no route matches issue #{} (labels: {:?})",
                    issue.number, issue.labels
                ))
            })?;

        let wf_name = route.workflow.as_deref().unwrap_or("");
        let template = state
            .config
            .resolve_workflow(wf_name)
            .ok_or_else(|| ApiError::Internal(format!("unknown workflow: {wf_name}")))?;

        let branch = state.config.branch_for_issue(&issue);
        let run_id = crate::state::generate_run_id();
        let plan =
            crate::planner::create_plan_with_config(&issue, &template, &branch, None, &run_id);
        let stages: Vec<String> = plan
            .stages
            .iter()
            .map(|s| s.kind_name().to_string())
            .collect();

        return Ok(Json(IssueDryRunResponse {
            issue_number: issue.number,
            issue_title: issue.title.clone(),
            route: route_name.to_string(),
            workflow: template.name.clone(),
            branch,
            model: state.config.effective_model(route).map(|s| s.to_string()),
            stages,
        })
        .into_response());
    }

    let model_override = query.model.clone();
    let workflow_override = query.workflow.clone();
    let config = state.config.clone();
    let state_dir = state.state_dir.clone();
    let gh = state.gh.clone();
    let git = state.git.clone();
    tokio::spawn(async move {
        match crate::runner::process_issue(
            number,
            &repo,
            &config,
            &routes,
            &state_dir,
            &rd,
            gh,
            git,
            model_override,
            vec![],
            None,
            workflow_override,
        )
        .await
        {
            Ok(run) => info!(run_id = run.run_id, "background issue run completed"),
            Err(e) => error!(error = ?e, issue = number, "background issue run failed"),
        }
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(AcceptedResponse {
            status: "accepted",
            message: "run queued — poll /runs or /runs/latest for results",
        }),
    )
        .into_response())
}

async fn trigger_pr(
    Path(number): Path<u64>,
    Query(query): Query<TriggerQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<Response, ApiError> {
    let (repo, rd, routes) =
        resolve_repo_for_api(query.repo.as_deref(), &state.config, &*state.git).await?;

    if query.dry_run.unwrap_or(false) {
        let pr = state
            .gh
            .fetch_pr(&repo, number)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        let (route_name, route) =
            RunnerConfig::match_pr_route_in(&routes, &pr).ok_or_else(|| {
                ApiError::BadRequest(format!(
                    "no route matches PR #{} (labels: {:?})",
                    pr.number, pr.labels
                ))
            })?;

        let wf_name = route.workflow.as_deref().unwrap_or("");
        let template = state
            .config
            .resolve_workflow(wf_name)
            .ok_or_else(|| ApiError::Internal(format!("unknown workflow: {wf_name}")))?;

        let branch = RunnerConfig::branch_for_pr(&pr);
        let run_id = crate::state::generate_run_id();
        let plan = crate::planner::create_pr_plan(&pr, &template, &branch, &run_id);
        let stages: Vec<String> = plan
            .stages
            .iter()
            .map(|s| s.kind_name().to_string())
            .collect();

        return Ok(Json(PrDryRunResponse {
            pr_number: pr.number,
            pr_title: pr.title.clone(),
            route: route_name.to_string(),
            workflow: template.name.clone(),
            branch,
            model: state.config.effective_model(route).map(|s| s.to_string()),
            stages,
        })
        .into_response());
    }

    let model_override = query.model.clone();
    let workflow_override = query.workflow.clone();
    let config = state.config.clone();
    let state_dir = state.state_dir.clone();
    let gh = state.gh.clone();
    let git = state.git.clone();
    tokio::spawn(async move {
        match crate::runner::process_pr(
            number,
            &repo,
            &config,
            &routes,
            &state_dir,
            &rd,
            gh,
            git,
            model_override,
            vec![],
            None,
            workflow_override,
        )
        .await
        {
            Ok(run) => info!(run_id = run.run_id, "background PR run completed"),
            Err(e) => error!(error = ?e, pr = number, "background PR run failed"),
        }
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(AcceptedResponse {
            status: "accepted",
            message: "run queued — poll /runs or /runs/latest for results",
        }),
    )
        .into_response())
}

async fn trigger_batch(State(state): State<Arc<AppState>>) -> Result<Response, ApiError> {
    let mut repos_resolved: Vec<(String, PathBuf, IndexMap<String, Route>)> = Vec::new();
    for (repo, entry_repo_dir, routes) in state.config.iter_repos() {
        let explicit_dir = entry_repo_dir
            .map(PathBuf::from)
            .or_else(|| state.config.global.repo_dir.as_ref().map(PathBuf::from));
        let rd = crate::isolation::find_or_clone_repo(repo, explicit_dir, &*state.git)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        repos_resolved.push((repo.to_string(), rd, routes.clone()));
    }

    let config = state.config.clone();
    let state_dir = state.state_dir.clone();
    let gh = state.gh.clone();
    let git = state.git.clone();
    tokio::spawn(async move {
        let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        for (repo, rd, routes) in &repos_resolved {
            let records = crate::runner::process_batch(
                repo,
                &config,
                &state_dir,
                rd,
                routes,
                &cancel_rx,
                gh.clone(),
                git.clone(),
            )
            .await;
            let succeeded = records
                .iter()
                .filter(|r| r.status == forza_core::RunStatus::Succeeded)
                .count();
            info!(
                repo = repo,
                processed = records.len(),
                succeeded = succeeded,
                "background batch complete"
            );
        }
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(AcceptedResponse {
            status: "accepted",
            message: "batch queued — poll /runs for results",
        }),
    )
        .into_response())
}

// --- Plan types ---

/// Request body for `POST /plans`.
#[derive(Deserialize)]
struct PlanCreateRequest {
    issues: Option<Vec<String>>,
    label: Option<String>,
    limit: Option<usize>,
    model: Option<String>,
    repo: Option<String>,
}

/// A single plan issue summary returned by `GET /plans`.
#[derive(Serialize)]
struct PlanSummary {
    number: u64,
    title: String,
    item_count: usize,
}

/// A single node in the plan DAG.
#[derive(Serialize)]
struct PlanNode {
    issue_number: u64,
    deps: Vec<u64>,
}

/// Response for `GET /plans/{number}`.
#[derive(Serialize)]
struct PlanDetailResponse {
    number: u64,
    title: String,
    body: String,
    nodes: Vec<PlanNode>,
}

/// Request body for `POST /plans/{number}/exec`.
#[derive(Deserialize, Default)]
struct PlanExecRequest {
    dry_run: Option<bool>,
    close: Option<bool>,
}

/// One item in a dry-run exec response.
#[derive(Serialize)]
struct PlanExecItem {
    issue_number: u64,
    deps: Vec<u64>,
}

/// Response for `POST /plans/{number}/exec` when `dry_run` is true.
#[derive(Serialize)]
struct PlanExecDryRunResponse {
    plan_number: u64,
    order: Vec<PlanExecItem>,
}

/// Per-issue status entry for `GET /plans/{number}/exec/status`.
#[derive(Serialize)]
struct PlanIssueStatus {
    issue_number: u64,
    github_state: String,
    status: String,
}

// --- Plan handlers ---

async fn create_plan(
    State(state): State<Arc<AppState>>,
    Json(body): Json<PlanCreateRequest>,
) -> Result<Response, ApiError> {
    let (repo, rd, routes) =
        resolve_repo_for_api(body.repo.as_deref(), &state.config, &*state.git).await?;

    // Fetch issues for the plan.
    let issues: Vec<crate::github::IssueCandidate> = if let Some(label) = &body.label {
        let mut issues = state
            .gh
            .fetch_issues_with_label(&repo, label)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        let limit = body.limit.unwrap_or(20);
        issues.truncate(limit);
        issues
    } else if let Some(issue_refs) = &body.issues {
        let mut result = Vec::new();
        for r in issue_refs {
            let n: u64 = r
                .parse()
                .map_err(|_| ApiError::BadRequest(format!("invalid issue number: {r}")))?;
            let issue = state
                .gh
                .fetch_issue(&repo, n)
                .await
                .map_err(|e| ApiError::Internal(e.to_string()))?;
            result.push(issue);
        }
        result
    } else {
        let limit = body.limit.unwrap_or(20);
        let issues = state
            .gh
            .fetch_eligible_issues(&repo, &[], limit)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        let lifecycle = ["forza:in-progress", "forza:complete", "forza:needs-human"];
        issues
            .into_iter()
            .filter(|i| !i.labels.iter().any(|l| lifecycle.contains(&l.as_str())))
            .collect()
    };

    if issues.is_empty() {
        return Err(ApiError::BadRequest("no issues to plan".into()));
    }

    let route_summary = build_route_summary(&routes);
    let issue_summaries = build_issue_summaries(&issues);
    let issue_refs_str = build_issue_refs(&issues);

    let preamble = forza_core::planner::make_preamble(&repo);
    let prompt = forza_core::planner::PROMPT_CMD_PLAN
        .replace("{preamble}", &preamble)
        .replace("{repo}", &repo)
        .replace("{routes}", &route_summary)
        .replace("{issues}", &issue_summaries)
        .replace("{issue_refs}", &issue_refs_str);

    let model = body
        .model
        .clone()
        .or_else(|| state.config.global.model.clone());
    let config = state.config.clone();

    tokio::spawn(async move {
        let agent: std::sync::Arc<dyn forza_core::AgentExecutor> =
            match config.global.agent.as_str() {
                "codex" => std::sync::Arc::new(crate::adapters::CodexAgentAdapter),
                _ => std::sync::Arc::new(crate::adapters::ClaudeAgentAdapter),
            };
        let allowed_tools: Vec<String> = vec![
            "Read".into(),
            "Glob".into(),
            "Grep".into(),
            "Bash(gh *)".into(),
        ];
        match agent
            .execute(
                "plan",
                &prompt,
                &rd,
                model.as_deref(),
                &[],
                None,
                None,
                &allowed_tools,
            )
            .await
        {
            Ok(_) => info!(repo = repo, "background plan creation completed"),
            Err(e) => error!(error = ?e, repo = repo, "background plan creation failed"),
        }
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(AcceptedResponse {
            status: "accepted",
            message: "plan creation queued",
        }),
    )
        .into_response())
}

async fn list_plans(
    Query(query): Query<TriggerQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<PlanSummary>>, ApiError> {
    let (repo, _, _) =
        resolve_repo_for_api(query.repo.as_deref(), &state.config, &*state.git).await?;

    let plan_issues = state
        .gh
        .fetch_issues_with_label(&repo, "forza:plan")
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let summaries = plan_issues
        .into_iter()
        .map(|issue| {
            let item_count = parse_plan_dag(&issue.body)
                .map(|dag| dag.len())
                .unwrap_or(0);
            PlanSummary {
                number: issue.number,
                title: issue.title,
                item_count,
            }
        })
        .collect();

    Ok(Json(summaries))
}

async fn get_plan(
    Path(number): Path<u64>,
    Query(query): Query<TriggerQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<PlanDetailResponse>, ApiError> {
    let (repo, _, _) =
        resolve_repo_for_api(query.repo.as_deref(), &state.config, &*state.git).await?;

    let issue = state
        .gh
        .fetch_issue(&repo, number)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let dag = parse_plan_dag(&issue.body)
        .map_err(|e| ApiError::BadRequest(format!("could not parse plan DAG: {e}")))?;

    let order = topological_sort(&dag)
        .map_err(|e| ApiError::Internal(format!("dependency sort failed: {e}")))?;

    let nodes = order
        .into_iter()
        .map(|n| PlanNode {
            issue_number: n,
            deps: dag.get(&n).cloned().unwrap_or_default(),
        })
        .collect();

    Ok(Json(PlanDetailResponse {
        number: issue.number,
        title: issue.title,
        body: issue.body,
        nodes,
    }))
}

async fn revise_plan(
    Path(number): Path<u64>,
    Query(query): Query<TriggerQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<Response, ApiError> {
    let (repo, rd, _) =
        resolve_repo_for_api(query.repo.as_deref(), &state.config, &*state.git).await?;

    let plan_issue = state
        .gh
        .fetch_issue(&repo, number)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    if !plan_issue.labels.iter().any(|l| l == "forza:plan") {
        return Err(ApiError::BadRequest(format!(
            "issue #{number} is not a plan issue (missing forza:plan label)"
        )));
    }

    let comments_text = if plan_issue.comments.is_empty() {
        "(no comments)".to_string()
    } else {
        plan_issue
            .comments
            .iter()
            .enumerate()
            .map(|(i, c)| format!("### Comment {}\n\n{}", i + 1, c))
            .collect::<Vec<_>>()
            .join("\n\n---\n\n")
    };

    let preamble = forza_core::planner::make_preamble(&repo);
    let prompt = forza_core::planner::PROMPT_CMD_PLAN_REVISE
        .replace("{preamble}", &preamble)
        .replace("{repo}", &repo)
        .replace("{plan_number}", &number.to_string())
        .replace("{plan_body}", &plan_issue.body)
        .replace("{comments}", &comments_text);

    let config = state.config.clone();

    tokio::spawn(async move {
        let agent: std::sync::Arc<dyn forza_core::AgentExecutor> =
            match config.global.agent.as_str() {
                "codex" => std::sync::Arc::new(crate::adapters::CodexAgentAdapter),
                _ => std::sync::Arc::new(crate::adapters::ClaudeAgentAdapter),
            };
        let allowed_tools: Vec<String> = vec![
            "Read".into(),
            "Glob".into(),
            "Grep".into(),
            "Bash(gh *)".into(),
        ];
        match agent
            .execute(
                "plan",
                &prompt,
                &rd,
                config.global.model.as_deref(),
                &[],
                None,
                None,
                &allowed_tools,
            )
            .await
        {
            Ok(_) => info!(plan = number, "background plan revision completed"),
            Err(e) => error!(error = ?e, plan = number, "background plan revision failed"),
        }
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(AcceptedResponse {
            status: "accepted",
            message: "plan revision queued",
        }),
    )
        .into_response())
}

async fn exec_plan(
    Path(number): Path<u64>,
    Query(query): Query<TriggerQuery>,
    State(state): State<Arc<AppState>>,
    body: Option<Json<PlanExecRequest>>,
) -> Result<Response, ApiError> {
    let req = body.map(|b| b.0).unwrap_or_default();
    let (repo, rd, routes) =
        resolve_repo_for_api(query.repo.as_deref(), &state.config, &*state.git).await?;

    let plan_issue = state
        .gh
        .fetch_issue(&repo, number)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    if !plan_issue.labels.iter().any(|l| l == "forza:plan") {
        return Err(ApiError::BadRequest(format!(
            "issue #{number} is not a plan issue (missing forza:plan label)"
        )));
    }

    let dag = parse_plan_dag(&plan_issue.body)
        .map_err(|e| ApiError::BadRequest(format!("could not parse plan DAG: {e}")))?;

    let order = topological_sort(&dag)
        .map_err(|e| ApiError::Internal(format!("dependency sort failed: {e}")))?;

    if req.dry_run.unwrap_or(false) {
        let items = order
            .into_iter()
            .map(|n| PlanExecItem {
                issue_number: n,
                deps: dag.get(&n).cloned().unwrap_or_default(),
            })
            .collect();
        return Ok(Json(PlanExecDryRunResponse {
            plan_number: number,
            order: items,
        })
        .into_response());
    }

    let config = state.config.clone();
    let state_dir = state.state_dir.clone();
    let gh = state.gh.clone();
    let git = state.git.clone();
    let close = req.close.unwrap_or(false);

    tokio::spawn(async move {
        let mut succeeded = 0u64;
        let mut failed = 0u64;
        let mut skipped: std::collections::HashSet<u64> = std::collections::HashSet::new();

        for issue_number in &order {
            if let Some(deps) = dag.get(issue_number)
                && deps.iter().any(|d| skipped.contains(d))
            {
                skipped.insert(*issue_number);
                continue;
            }

            match crate::runner::process_issue(
                *issue_number,
                &repo,
                &config,
                &routes,
                &state_dir,
                &rd,
                gh.clone(),
                git.clone(),
                None,
                vec![],
                None,
                None,
            )
            .await
            {
                Ok(run) => {
                    if run.status == forza_core::RunStatus::Succeeded {
                        succeeded += 1;
                    } else {
                        failed += 1;
                        skipped.insert(*issue_number);
                    }
                }
                Err(e) => {
                    error!(error = ?e, issue = issue_number, "plan exec issue failed");
                    failed += 1;
                    skipped.insert(*issue_number);
                }
            }
        }

        info!(
            plan = number,
            succeeded = succeeded,
            failed = failed,
            "background plan exec completed"
        );

        if close {
            let summary = format!(
                "Plan execution complete: {succeeded} succeeded, {failed} failed, {} skipped.",
                skipped.len().saturating_sub(failed as usize)
            );
            if let Err(e) = gh.comment_on_issue(&repo, number, &summary).await {
                error!(error = ?e, plan = number, "failed to post plan summary comment");
            }
            if let Err(e) = gh.close_issue(&repo, number).await {
                error!(error = ?e, plan = number, "failed to close plan issue");
            }
        }
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(AcceptedResponse {
            status: "accepted",
            message: "plan execution queued",
        }),
    )
        .into_response())
}

async fn plan_exec_status(
    Path(number): Path<u64>,
    Query(query): Query<TriggerQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<PlanIssueStatus>>, ApiError> {
    let (repo, _, _) =
        resolve_repo_for_api(query.repo.as_deref(), &state.config, &*state.git).await?;

    let plan_issue = state
        .gh
        .fetch_issue(&repo, number)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let dag = parse_plan_dag(&plan_issue.body)
        .map_err(|e| ApiError::BadRequest(format!("could not parse plan DAG: {e}")))?;

    let order = topological_sort(&dag)
        .map_err(|e| ApiError::Internal(format!("dependency sort failed: {e}")))?;

    let all_runs = crate::state::load_all_runs(&state.state_dir);
    let mut failed_issues: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut statuses = Vec::new();

    for issue_number in &order {
        let github_state = state
            .gh
            .fetch_issue_state(&repo, *issue_number)
            .await
            .unwrap_or_else(|_| "unknown".to_string());

        // Check if issue has forza:complete label.
        let issue_labels = state
            .gh
            .fetch_issue(&repo, *issue_number)
            .await
            .map(|i| i.labels)
            .unwrap_or_default();

        let status = if issue_labels.iter().any(|l| l == "forza:complete") {
            "complete".to_string()
        } else if all_runs
            .iter()
            .any(|r| r.issue_number == *issue_number && r.status == crate::state::RunStatus::Failed)
        {
            failed_issues.insert(*issue_number);
            "failed".to_string()
        } else if let Some(deps) = dag.get(issue_number)
            && deps.iter().any(|d| failed_issues.contains(d))
        {
            "blocked".to_string()
        } else {
            "pending".to_string()
        };

        statuses.push(PlanIssueStatus {
            issue_number: *issue_number,
            github_state,
            status,
        });
    }

    Ok(Json(statuses))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::path::Path;

    struct NoopGit;

    #[async_trait]
    impl crate::git::GitClient for NoopGit {
        async fn fetch(&self, _: &Path) -> crate::error::Result<()> {
            unimplemented!()
        }
        async fn worktree_add(&self, _: &Path, _: &str, _: &str) -> crate::error::Result<PathBuf> {
            unimplemented!()
        }
        async fn worktree_remove(&self, _: &Path, _: &Path, _: bool) -> crate::error::Result<()> {
            unimplemented!()
        }
        async fn remote_url(&self, _: &Path) -> crate::error::Result<String> {
            unimplemented!()
        }
        async fn ref_exists(&self, _: &Path, _: &str) -> crate::error::Result<bool> {
            unimplemented!()
        }
        async fn has_changes(&self, _: &Path) -> crate::error::Result<bool> {
            unimplemented!()
        }
        async fn stage_tracked(&self, _: &Path) -> crate::error::Result<()> {
            unimplemented!()
        }
        async fn stage_path(&self, _: &Path, _: &str) -> crate::error::Result<()> {
            unimplemented!()
        }
        async fn commit(&self, _: &Path, _: &str) -> crate::error::Result<()> {
            unimplemented!()
        }
        async fn rebase(&self, _: &Path, _: &str) -> crate::error::Result<bool> {
            unimplemented!()
        }
        async fn rebase_abort(&self, _: &Path) -> crate::error::Result<()> {
            unimplemented!()
        }
        async fn diff_stat(&self, _: &Path, _: &str) -> crate::error::Result<String> {
            unimplemented!()
        }
        async fn push(&self, _: &Path, _: &str) -> crate::error::Result<()> {
            unimplemented!()
        }
        async fn push_force(&self, _: &Path, _: &str) -> crate::error::Result<()> {
            unimplemented!()
        }
        async fn create_branch_from(&self, _: &Path, _: &str, _: &str) -> crate::error::Result<()> {
            unimplemented!()
        }
        async fn default_branch(&self, _: &Path) -> crate::error::Result<String> {
            unimplemented!()
        }
        async fn version(&self) -> crate::error::Result<String> {
            unimplemented!()
        }
    }

    fn multi_repo_config() -> RunnerConfig {
        toml::from_str(
            r#"
[global]

[repos."owner/repo-a".routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"

[repos."owner/repo-b".routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"
"#,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn resolve_repo_for_api_multi_no_param_errors() {
        let config = multi_repo_config();
        let git = NoopGit;
        let err = resolve_repo_for_api(None, &config, &git).await.unwrap_err();
        assert!(matches!(err, ApiError::BadRequest(_)));
        if let ApiError::BadRequest(msg) = err {
            assert!(msg.contains("multiple repos configured"));
        }
    }

    #[tokio::test]
    async fn resolve_repo_for_api_multi_unknown_repo_errors() {
        let config = multi_repo_config();
        let git = NoopGit;
        let err = resolve_repo_for_api(Some("owner/unknown"), &config, &git)
            .await
            .unwrap_err();
        assert!(matches!(err, ApiError::BadRequest(_)));
        if let ApiError::BadRequest(msg) = err {
            assert!(msg.contains("not found in config"));
        }
    }
}
