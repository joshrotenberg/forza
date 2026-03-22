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

    // Spawn background task; the orchestrator generates and persists its own run_id.
    let config = state.config.clone();
    let state_dir = state.state_dir.clone();
    let gh = state.gh.clone();
    let git = state.git.clone();
    tokio::spawn(async move {
        match crate::orchestrator::process_issue_with_config(
            number, &repo, &routes, &config, &state_dir, &rd, &*gh, &*git,
        )
        .await
        {
            Ok(record) => info!(run_id = record.run_id, "background issue run completed"),
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

    let config = state.config.clone();
    let state_dir = state.state_dir.clone();
    let gh = state.gh.clone();
    let git = state.git.clone();
    tokio::spawn(async move {
        match crate::orchestrator::process_pr_with_config(
            number, &repo, &routes, &config, &state_dir, &rd, &*gh, &*git,
        )
        .await
        {
            Ok(record) => info!(run_id = record.run_id, "background PR run completed"),
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
            let records = crate::orchestrator::process_batch_for_repo(
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
                .filter(|r| r.status == crate::state::RunStatus::Succeeded)
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
