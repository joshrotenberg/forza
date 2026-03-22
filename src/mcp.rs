//! MCP server — exposes forza capabilities over the Model Context Protocol.
//!
//! Three tool groups share an [`AppState`] holding the loaded config and the
//! run-state directory:
//!
//! - **Runner** (`issue_run`, `pr_run`, `run_batch`, `dry_run_issue`): single-shot
//!   operations that mirror the CLI subcommands.
//! - **Status** (`status_latest`, `status_list`, `status_get`, `status_summary`,
//!   `status_find_issue`): read persisted run records from disk.
//! - **Config** (`config_show`, `config_validate`): inspect or validate
//!   configuration.

use std::path::PathBuf;

use indexmap::IndexMap;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tower_mcp::extract::{Json, State};
use tower_mcp::{CallToolResult, McpRouter, StdioTransport, ToolBuilder};

use crate::config::{Route, RunnerConfig};

type RepoResolution = (String, Option<PathBuf>, IndexMap<String, Route>);

// ── Shared state ─────────────────────────────────────────────────────────────

/// State shared by every MCP tool handler.
#[derive(Clone)]
pub struct AppState {
    config: Arc<RunnerConfig>,
    state_dir: PathBuf,
    gh: Arc<dyn crate::github::GitHubClient>,
    git: Arc<dyn crate::git::GitClient>,
}

impl AppState {
    /// Create a new `AppState`.
    pub fn new(
        config: RunnerConfig,
        state_dir: PathBuf,
        gh: Arc<dyn crate::github::GitHubClient>,
        git: Arc<dyn crate::git::GitClient>,
    ) -> Self {
        Self {
            config: Arc::new(config),
            state_dir,
            gh,
            git,
        }
    }
}

// ── Input types ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
struct IssueRunInput {
    /// Issue number to process.
    number: u64,
    /// Repository (owner/name). Required when multiple repos are configured.
    repo: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PrRunInput {
    /// PR number to process.
    number: u64,
    /// Repository (owner/name). Required when multiple repos are configured.
    repo: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DryRunIssueInput {
    /// Issue number to show the plan for.
    number: u64,
    /// Repository (owner/name). Required when multiple repos are configured.
    repo: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct StatusGetInput {
    /// Run ID to retrieve.
    run_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct StatusFindIssueInput {
    /// Issue number to look up.
    issue_number: u64,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ConfigValidateInput {
    /// Path to the config file to validate.
    path: String,
}

// ── Helper ────────────────────────────────────────────────────────────────────

/// Resolve the repo slug, repo dir, and route map from the config.
///
/// Handles both single-repo (legacy) and multi-repo modes.
fn resolve_repo(config: &RunnerConfig, repo: Option<&str>) -> Result<RepoResolution, String> {
    let repos = config.iter_repos();
    let (repo_str, entry_dir, routes) = if repos.len() == 1 {
        repos
            .into_iter()
            .next()
            .ok_or_else(|| "no repos configured".to_string())?
    } else {
        match repo {
            Some(r) => match repos.into_iter().find(|(s, _, _)| *s == r) {
                Some(entry) => entry,
                None => return Err(format!("repo '{r}' not found in config")),
            },
            None => return Err("multiple repos configured — specify the 'repo' field".to_string()),
        }
    };

    let explicit_dir = entry_dir
        .map(PathBuf::from)
        .or_else(|| config.global.repo_dir.as_ref().map(PathBuf::from));

    Ok((repo_str.to_string(), explicit_dir, routes.clone()))
}

// ── Router ────────────────────────────────────────────────────────────────────

/// Build the [`McpRouter`] with all three tool groups.
pub fn build_router(state: AppState) -> McpRouter {
    let s = Arc::new(state);

    // ── Runner: issue_run ─────────────────────────────────────────────────────
    let issue_run = ToolBuilder::new("issue_run")
        .description("Process a single GitHub issue through the full pipeline")
        .extractor_handler(
            s.clone(),
            |State(app): State<Arc<AppState>>, Json(input): Json<IssueRunInput>| async move {
                let (repo, explicit_dir, routes) =
                    match resolve_repo(&app.config, input.repo.as_deref()) {
                        Ok(r) => r,
                        Err(e) => return Ok(CallToolResult::text(format!("error: {e}"))),
                    };
                let rd = match crate::isolation::find_or_clone_repo(&repo, explicit_dir, &*app.git)
                    .await
                {
                    Ok(p) => p,
                    Err(e) => return Ok(CallToolResult::text(format!("error: {e}"))),
                };
                match crate::orchestrator::process_issue_with_config(
                    input.number,
                    &repo,
                    &routes,
                    &app.config,
                    &app.state_dir,
                    &rd,
                    &*app.gh,
                    &*app.git,
                )
                .await
                {
                    Ok(record) => Ok(CallToolResult::text(
                        serde_json::to_string_pretty(&record).unwrap_or_default(),
                    )),
                    Err(e) => Ok(CallToolResult::text(format!("error: {e}"))),
                }
            },
        )
        .build();

    // ── Runner: pr_run ────────────────────────────────────────────────────────
    let pr_run = ToolBuilder::new("pr_run")
        .description("Process a single GitHub PR through the full pipeline")
        .extractor_handler(
            s.clone(),
            |State(app): State<Arc<AppState>>, Json(input): Json<PrRunInput>| async move {
                let (repo, explicit_dir, routes) =
                    match resolve_repo(&app.config, input.repo.as_deref()) {
                        Ok(r) => r,
                        Err(e) => return Ok(CallToolResult::text(format!("error: {e}"))),
                    };
                let rd = match crate::isolation::find_or_clone_repo(&repo, explicit_dir, &*app.git)
                    .await
                {
                    Ok(p) => p,
                    Err(e) => return Ok(CallToolResult::text(format!("error: {e}"))),
                };
                match crate::orchestrator::process_pr_with_config(
                    input.number,
                    &repo,
                    &routes,
                    &app.config,
                    &app.state_dir,
                    &rd,
                    &*app.gh,
                    &*app.git,
                )
                .await
                {
                    Ok(record) => Ok(CallToolResult::text(
                        serde_json::to_string_pretty(&record).unwrap_or_default(),
                    )),
                    Err(e) => Ok(CallToolResult::text(format!("error: {e}"))),
                }
            },
        )
        .build();

    // ── Runner: run_batch ─────────────────────────────────────────────────────
    let run_batch = ToolBuilder::new("run_batch")
        .description("Poll for all eligible issues across configured repos and process them once")
        .extractor_handler(s.clone(), |State(app): State<Arc<AppState>>| async move {
            let config = &app.config;
            // Collect repo info upfront to avoid holding references across awaits.
            let repos: Vec<(String, Option<PathBuf>, IndexMap<String, Route>)> = config
                .iter_repos()
                .into_iter()
                .map(|(repo, dir, routes)| {
                    (repo.to_string(), dir.map(PathBuf::from), routes.clone())
                })
                .collect();

            let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
            let mut all_records = Vec::new();

            for (repo, explicit_dir, routes) in repos {
                let explicit_dir =
                    explicit_dir.or_else(|| config.global.repo_dir.as_ref().map(PathBuf::from));
                let rd = match crate::isolation::find_or_clone_repo(&repo, explicit_dir, &*app.git)
                    .await
                {
                    Ok(p) => p,
                    Err(e) => return Ok(CallToolResult::text(format!("error: {e}"))),
                };
                let mut records = crate::orchestrator::process_batch_for_repo(
                    &repo,
                    config,
                    &app.state_dir,
                    &rd,
                    &routes,
                    &cancel_rx,
                    app.gh.clone(),
                    app.git.clone(),
                )
                .await;
                all_records.append(&mut records);
            }

            Ok(CallToolResult::text(
                serde_json::to_string_pretty(&all_records).unwrap_or_default(),
            ))
        })
        .build();

    // ── Runner: dry_run_issue ─────────────────────────────────────────────────
    let dry_run_issue = ToolBuilder::new("dry_run_issue")
        .description("Show the execution plan for an issue without running it")
        .extractor_handler(
            s.clone(),
            |State(app): State<Arc<AppState>>,
             Json(input): Json<DryRunIssueInput>| async move {
                let config = &app.config;
                let (repo, _, routes) =
                    match resolve_repo(config, input.repo.as_deref()) {
                        Ok(r) => r,
                        Err(e) => return Ok(CallToolResult::text(format!("error: {e}"))),
                    };
                let issue =
                    match app.gh.fetch_issue(&repo, input.number).await {
                        Ok(i) => i,
                        Err(e) => {
                            return Ok(CallToolResult::text(format!("error: {e}")))
                        }
                    };
                let (route_name, route) =
                    match RunnerConfig::match_route_in(&routes, &issue) {
                        Some(r) => r,
                        None => {
                            return Ok(CallToolResult::text(format!(
                                "no route matches issue #{} (labels: {:?})",
                                issue.number, issue.labels
                            )))
                        }
                    };
                let wf_name = route.workflow.as_deref().unwrap_or("");
                let template = match config.resolve_workflow(wf_name) {
                    Some(t) => t,
                    None => {
                        return Ok(CallToolResult::text(format!(
                            "unknown workflow: {wf_name}"
                        )))
                    }
                };
                let branch = config.branch_for_issue(&issue);
                let run_id = crate::state::generate_run_id();
                let plan = crate::planner::create_plan_with_config(
                    &issue, &template, &branch, None, &run_id,
                );

                let mut lines = vec![
                    format!("Issue:    #{} — {}", issue.number, issue.title),
                    format!("Route:    {route_name}"),
                    format!("Workflow: {}", template.name),
                    format!("Branch:   {branch}"),
                ];
                if let Some(model) = config.effective_model(route) {
                    lines.push(format!("Model:    {model}"));
                }
                lines.push("Stages:".to_string());
                for (i, stage) in plan.stages.iter().enumerate() {
                    let optional = if stage.optional { " (optional)" } else { "" };
                    lines.push(format!("  {}. {}{optional}", i + 1, stage.kind_name()));
                }
                if let Some(est) =
                    crate::state::estimate_cost(&template.name, &app.state_dir)
                {
                    lines.push(format!(
                        "Estimated cost: ${:.2} - ${:.2} (avg ${:.2}, based on {} previous {} runs)",
                        est.min, est.max, est.avg, est.count, est.workflow
                    ));
                }
                Ok(CallToolResult::text(lines.join("\n")))
            },
        )
        .build();

    // ── Status: status_latest ─────────────────────────────────────────────────
    let status_latest = ToolBuilder::new("status_latest")
        .description("Get the most recent run record")
        .extractor_handler(s.clone(), |State(app): State<Arc<AppState>>| async move {
            match crate::state::load_latest(&app.state_dir) {
                Some(record) => Ok(CallToolResult::text(
                    serde_json::to_string_pretty(&record).unwrap_or_default(),
                )),
                None => Ok(CallToolResult::text("no runs found".to_string())),
            }
        })
        .build();

    // ── Status: status_list ───────────────────────────────────────────────────
    let status_list = ToolBuilder::new("status_list")
        .description("List all run records sorted newest-first")
        .extractor_handler(s.clone(), |State(app): State<Arc<AppState>>| async move {
            let records = crate::state::load_all_runs(&app.state_dir);
            Ok(CallToolResult::text(
                serde_json::to_string_pretty(&records).unwrap_or_default(),
            ))
        })
        .build();

    // ── Status: status_get ────────────────────────────────────────────────────
    let status_get = ToolBuilder::new("status_get")
        .description("Get a specific run record by ID")
        .extractor_handler(
            s.clone(),
            |State(app): State<Arc<AppState>>, Json(input): Json<StatusGetInput>| async move {
                match crate::state::load_run(&input.run_id, &app.state_dir) {
                    Some(record) => Ok(CallToolResult::text(
                        serde_json::to_string_pretty(&record).unwrap_or_default(),
                    )),
                    None => Ok(CallToolResult::text(format!(
                        "run not found: {}",
                        input.run_id
                    ))),
                }
            },
        )
        .build();

    // ── Status: status_summary ────────────────────────────────────────────────
    let status_summary = ToolBuilder::new("status_summary")
        .description("Get per-workflow aggregate statistics across all runs")
        .extractor_handler(s.clone(), |State(app): State<Arc<AppState>>| async move {
            #[derive(Serialize)]
            struct SummaryRow {
                workflow: String,
                total_runs: usize,
                succeeded: usize,
                failed: usize,
                min_cost: Option<f64>,
                max_cost: Option<f64>,
                avg_cost: Option<f64>,
            }
            let rows: Vec<SummaryRow> = crate::state::summarize_by_workflow(&app.state_dir)
                .into_iter()
                .map(|s| SummaryRow {
                    workflow: s.workflow,
                    total_runs: s.total_runs,
                    succeeded: s.succeeded,
                    failed: s.failed,
                    min_cost: s.min_cost,
                    max_cost: s.max_cost,
                    avg_cost: s.avg_cost,
                })
                .collect();
            Ok(CallToolResult::text(
                serde_json::to_string_pretty(&rows).unwrap_or_default(),
            ))
        })
        .build();

    // ── Status: status_find_issue ─────────────────────────────────────────────
    let status_find_issue = ToolBuilder::new("status_find_issue")
        .description("Find the most recent run for a given issue number")
        .extractor_handler(
            s.clone(),
            |State(app): State<Arc<AppState>>, Json(input): Json<StatusFindIssueInput>| async move {
                match crate::state::find_latest_run_for_issue(input.issue_number, &app.state_dir) {
                    Some(record) => Ok(CallToolResult::text(
                        serde_json::to_string_pretty(&record).unwrap_or_default(),
                    )),
                    None => Ok(CallToolResult::text(format!(
                        "no run found for issue #{}",
                        input.issue_number
                    ))),
                }
            },
        )
        .build();

    // ── Config: config_show ───────────────────────────────────────────────────
    let config_show = ToolBuilder::new("config_show")
        .description("Return the currently loaded runner configuration as JSON")
        .extractor_handler(s.clone(), |State(app): State<Arc<AppState>>| async move {
            Ok(CallToolResult::text(
                serde_json::to_string_pretty(&*app.config).unwrap_or_default(),
            ))
        })
        .build();

    // ── Config: config_validate ───────────────────────────────────────────────
    let config_validate = ToolBuilder::new("config_validate")
        .description("Parse and validate a forza config file, returning any errors")
        .extractor_handler(
            s.clone(),
            |State(_app): State<Arc<AppState>>, Json(input): Json<ConfigValidateInput>| async move {
                let path = std::path::Path::new(&input.path);
                match RunnerConfig::from_file(path) {
                    Ok(config) => Ok(CallToolResult::text(format!(
                        "config is valid\nrepo: {:?}\nroutes: {}",
                        config.global.repo,
                        config.routes.len()
                    ))),
                    Err(e) => Ok(CallToolResult::text(format!("invalid config: {e}"))),
                }
            },
        )
        .build();

    McpRouter::new()
        .server_info("forza", env!("CARGO_PKG_VERSION"))
        .instructions(
            "Forza autonomous GitHub issue runner. \
             Use runner tools to process issues/PRs, \
             status tools to query run history, \
             and config tools to inspect or validate configuration.",
        )
        .tool(issue_run)
        .tool(pr_run)
        .tool(run_batch)
        .tool(dry_run_issue)
        .tool(status_latest)
        .tool(status_list)
        .tool(status_get)
        .tool(status_summary)
        .tool(status_find_issue)
        .tool(config_show)
        .tool(config_validate)
}

/// Start the MCP server on stdio transport.
pub async fn serve(
    config: RunnerConfig,
    state_dir: PathBuf,
    gh: Arc<dyn crate::github::GitHubClient>,
    git: Arc<dyn crate::git::GitClient>,
) -> crate::error::Result<()> {
    let state = AppState::new(config, state_dir, gh, git);
    let router = build_router(state);
    StdioTransport::new(router)
        .run()
        .await
        .map_err(|e| crate::error::Error::State(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn single_repo_config() -> RunnerConfig {
        toml::from_str(
            r#"
[global]
repo = "owner/repo"

[routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"
"#,
        )
        .unwrap()
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

    #[test]
    fn resolve_repo_single_repo_succeeds() {
        let config = single_repo_config();
        let (repo, _dir, _routes) = resolve_repo(&config, None).unwrap();
        assert_eq!(repo, "owner/repo");
    }

    #[test]
    fn resolve_repo_multi_no_param_errors() {
        let config = multi_repo_config();
        let err = resolve_repo(&config, None).unwrap_err();
        assert!(err.contains("multiple repos configured"));
    }

    #[test]
    fn resolve_repo_multi_with_param_succeeds() {
        let config = multi_repo_config();
        let (repo, _dir, _routes) = resolve_repo(&config, Some("owner/repo-a")).unwrap();
        assert_eq!(repo, "owner/repo-a");
    }

    #[test]
    fn resolve_repo_multi_unknown_param_errors() {
        let config = multi_repo_config();
        let err = resolve_repo(&config, Some("owner/unknown")).unwrap_err();
        assert!(err.contains("not found in config"));
    }
}
