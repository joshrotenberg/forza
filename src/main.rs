use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use tracing::info;

/// Autonomous GitHub issue runner — turns issues into pull requests.
#[derive(Debug, Parser)]
#[command(
    name = "forza",
    version,
    about,
    long_about = "Autonomous GitHub issue runner that processes issues through\n\
        configurable workflow templates (plan -> implement -> test -> PR).\n\n\
        Agent-agnostic: uses Claude by default, pluggable for other agents."
)]
struct Cli {
    /// Path to config file.
    #[arg(long, short, default_value = "forza.toml", global = true)]
    config: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Initialize forza: create GitHub labels and generate a starter config.
    Init(InitArgs),
    /// Process a single issue by number.
    Issue(IssueArgs),
    /// Process a single PR by number.
    Pr(PrArgs),
    /// Run once — poll for eligible issues and process them.
    Run(RunArgs),
    /// Watch mode — continuously poll and process issues.
    Watch(WatchArgs),
    /// Show run history and status.
    Status(StatusArgs),
    /// Re-run failed stages with error context.
    Fix(FixArgs),
    /// Remove worktrees or run state.
    Clean(CleanArgs),
    /// Serve the REST API.
    Serve(ServeArgs),
    /// Start the MCP server (stdio transport).
    Mcp(McpArgs),
}

#[derive(Debug, Parser)]
struct InitArgs {
    /// Repository in owner/name format (e.g. acme/myrepo).
    #[arg(long)]
    repo: String,
    /// Output path for the generated config file.
    #[arg(long, default_value = "forza.toml")]
    output: std::path::PathBuf,
}

#[derive(Debug, Parser)]
struct FixArgs {
    /// Run ID to fix (default: latest run).
    #[arg(long)]
    run: Option<String>,
    /// Issue number to fix (finds latest run for this issue).
    #[arg(long)]
    issue: Option<u64>,
}

#[derive(Debug, Parser)]
struct IssueArgs {
    /// Issue number to process.
    number: u64,
    /// Repository to process (owner/name). Required when multiple repos are configured.
    #[arg(long)]
    repo: Option<String>,
    /// Repository directory (default: current directory).
    #[arg(long)]
    repo_dir: Option<PathBuf>,
    /// Dry run — show the plan without executing.
    #[arg(long)]
    dry_run: bool,
    /// Override the model for every stage in this run (e.g. claude-opus-4-6).
    #[arg(long)]
    model: Option<String>,
    /// Add a skill file for every stage in this run (repeatable).
    #[arg(long, action = clap::ArgAction::Append)]
    skill: Vec<String>,
}

#[derive(Debug, Parser)]
struct PrArgs {
    /// PR number to process.
    number: u64,
    /// Repository to process (owner/name). Required when multiple repos are configured.
    #[arg(long)]
    repo: Option<String>,
    /// Repository directory (default: current directory).
    #[arg(long)]
    repo_dir: Option<PathBuf>,
    /// Dry run — show the plan without executing.
    #[arg(long)]
    dry_run: bool,
    /// Override the model for every stage in this run (e.g. claude-opus-4-6).
    #[arg(long)]
    model: Option<String>,
    /// Add a skill file for every stage in this run (repeatable).
    #[arg(long, action = clap::ArgAction::Append)]
    skill: Vec<String>,
}

#[derive(Debug, Parser)]
struct RunArgs {
    /// Repository directory.
    #[arg(long)]
    repo_dir: Option<PathBuf>,
}

#[derive(Debug, Parser)]
struct WatchArgs {
    /// Override poll interval in seconds (uses per-route intervals by default).
    #[arg(long)]
    interval: Option<u64>,
    /// Only run a specific route.
    #[arg(long)]
    route: Option<String>,
    /// Repository directory.
    #[arg(long)]
    repo_dir: Option<PathBuf>,
}

#[derive(Debug, Parser)]
struct StatusArgs {
    /// Show a specific run by ID.
    #[arg(long)]
    run_id: Option<String>,
    /// Show all runs as a history table (sorted newest first).
    #[arg(long)]
    all: bool,
    /// Show per-workflow aggregate summary.
    #[arg(long)]
    summary: bool,
}

#[derive(Debug, Parser)]
struct McpArgs {}

#[derive(Debug, Parser)]
struct CleanArgs {
    /// Repository directory (default: current directory).
    #[arg(long)]
    repo_dir: Option<PathBuf>,
    /// Remove run state files instead of worktrees.
    #[arg(long)]
    runs: bool,
    /// Remove only worktrees older than the configured threshold (see --days).
    #[arg(long)]
    stale: bool,
    /// Age threshold in days for --stale (overrides the configured default).
    #[arg(long)]
    days: Option<u64>,
    /// Print what would be removed without acting.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Parser)]
struct ServeArgs {
    /// Host address to bind to.
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    /// Port to listen on.
    #[arg(long, default_value_t = 8080)]
    port: u16,
    /// Repository directory (passed to repo resolution).
    #[arg(long)]
    repo_dir: Option<PathBuf>,
}

fn state_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".forza")
        .join("runs")
}

fn load_config(path: &std::path::Path) -> Result<forza::RunnerConfig, ExitCode> {
    match forza::RunnerConfig::from_file(path) {
        Ok(c) => Ok(c),
        Err(e) => {
            eprintln!("error loading config {}: {e}", path.display());
            Err(ExitCode::FAILURE)
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    if let Command::Init(args) = cli.command {
        return cmd_init(args).await;
    }

    let config = match load_config(&cli.config) {
        Ok(c) => c,
        Err(code) => return code,
    };

    if !matches!(
        cli.command,
        Command::Status(_) | Command::Clean(_) | Command::Serve(_) | Command::Mcp(_)
    ) && let Err(e) = forza::deps::validate_dependencies(&config.global.agent).await
    {
        eprintln!("error: {e}");
        return ExitCode::FAILURE;
    }

    match cli.command {
        Command::Init(_) => unreachable!(),
        Command::Issue(args) => cmd_issue(args, &config).await,
        Command::Pr(args) => cmd_pr(args, &config).await,
        Command::Run(args) => cmd_run(args, &config).await,
        Command::Watch(args) => cmd_watch(args, &config).await,
        Command::Status(args) => cmd_status(args),
        Command::Fix(args) => cmd_fix(args, &config).await,
        Command::Clean(args) => cmd_clean(args, &config).await,
        Command::Serve(args) => cmd_serve(args, config).await,
        Command::Mcp(_args) => cmd_mcp(&config).await,
    }
}

async fn cmd_init(args: InitArgs) -> ExitCode {
    // Forza lifecycle labels.
    let labels: &[(&str, &str, &str)] = &[
        (
            "forza:ready",
            "0075ca",
            "Opt-in gate: process this issue with forza",
        ),
        (
            "forza:in-progress",
            "e4e669",
            "Forza is currently processing this issue",
        ),
        (
            "forza:complete",
            "0e8a16",
            "Forza successfully processed this issue",
        ),
        (
            "forza:failed",
            "d73a4a",
            "Forza encountered an error processing this issue",
        ),
        (
            "forza:needs-human",
            "c2e0c6",
            "Retry budget exhausted, needs human review",
        ),
    ];

    println!("Creating forza labels in {}...", args.repo);
    for (name, color, description) in labels {
        match forza::github::create_label(&args.repo, name, color, description).await {
            Ok(()) => println!("  label: {name}"),
            Err(e) => {
                eprintln!("error creating label {name}: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    // Detect language for validation commands.
    let validation_commands = if std::path::Path::new("Cargo.toml").exists() {
        "commands = [\n    \"cargo fmt --all -- --check\",\n    \"cargo clippy --all-targets -- -D warnings\",\n    \"cargo test --lib\",\n]"
    } else if std::path::Path::new("package.json").exists() {
        "commands = [\n    \"npm run lint\",\n    \"npm test\",\n]"
    } else {
        "# commands = []  # add your validation commands here"
    };

    let config_content = format!(
        r#"# forza.toml — generated by `forza init`
# Edit this file to customize your forza configuration.

[global]
repo = "{repo}"
gate_label = "forza:ready"
branch_pattern = "automation/{{issue}}-{{slug}}"

[security]
authorization_level = "contributor"

[validation]
{validation}

[routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"

[routes.features]
type = "issue"
label = "enhancement"
workflow = "feature"
"#,
        repo = args.repo,
        validation = validation_commands,
    );

    if let Err(e) = std::fs::write(&args.output, &config_content) {
        eprintln!("error writing {}: {e}", args.output.display());
        return ExitCode::FAILURE;
    }

    println!("Created {}", args.output.display());
    ExitCode::SUCCESS
}

/// Resolve which repo, repo_dir, and routes to use for a single-issue command.
///
/// Checks `args.repo` when multiple repos are configured; errors if ambiguous.
async fn resolve_repo<'a>(
    args_repo: Option<&str>,
    args_repo_dir: &Option<PathBuf>,
    config: &'a forza::RunnerConfig,
) -> Result<
    (
        String,
        PathBuf,
        &'a std::collections::HashMap<String, forza::config::Route>,
    ),
    ExitCode,
> {
    let repos = config.iter_repos();
    let (repo_str, entry_repo_dir, routes) = if repos.len() == 1 {
        repos.into_iter().next().unwrap()
    } else {
        match args_repo {
            Some(r) => match repos.into_iter().find(|(repo, _, _)| *repo == r) {
                Some(entry) => entry,
                None => {
                    eprintln!("error: repo '{r}' not found in config");
                    return Err(ExitCode::FAILURE);
                }
            },
            None => {
                eprintln!("error: multiple repos configured — use --repo to specify which one");
                return Err(ExitCode::FAILURE);
            }
        }
    };

    // The explicit_dir priority: per-repo entry_repo_dir > CLI arg > global config repo_dir.
    let explicit_dir = entry_repo_dir
        .map(PathBuf::from)
        .or_else(|| args_repo_dir.clone())
        .or_else(|| config.global.repo_dir.as_ref().map(PathBuf::from));

    let rd = match forza::isolation::find_or_clone_repo(repo_str, explicit_dir).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return Err(ExitCode::FAILURE);
        }
    };

    Ok((repo_str.to_string(), rd, routes))
}

async fn cmd_issue(args: IssueArgs, config: &forza::RunnerConfig) -> ExitCode {
    let sd = state_dir();

    let (repo, rd, routes) = match resolve_repo(args.repo.as_deref(), &args.repo_dir, config).await
    {
        Ok(r) => r,
        Err(code) => return code,
    };

    if args.dry_run {
        let issue = match forza::github::fetch_issue(&repo, args.number).await {
            Ok(i) => i,
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        };

        // Match route.
        let (route_name, route) = match forza::RunnerConfig::match_route_in(routes, &issue) {
            Some(r) => r,
            None => {
                eprintln!(
                    "no route matches issue #{} (labels: {:?})",
                    issue.number, issue.labels
                );
                return ExitCode::FAILURE;
            }
        };

        let wf_name = route.workflow.as_deref().unwrap_or("");
        let template = match config.resolve_workflow(wf_name) {
            Some(t) => t,
            None => {
                eprintln!("unknown workflow: {wf_name}");
                return ExitCode::FAILURE;
            }
        };

        let branch = config.branch_for_issue(&issue);
        let run_id = forza::state::generate_run_id();
        let plan = forza::planner::create_plan(&issue, &template, &branch, None, &run_id);

        println!("Issue:    #{} — {}", issue.number, issue.title);
        println!("Route:    {route_name}");
        println!("Workflow: {}", template.name);
        println!("Branch:   {branch}");
        let effective_model = args
            .model
            .as_deref()
            .or_else(|| config.effective_model(route));
        if let Some(model) = effective_model {
            println!("Model:    {model}");
        }
        println!("Stages:");
        for (i, stage) in plan.stages.iter().enumerate() {
            let optional = if stage.optional { " (optional)" } else { "" };
            println!("  {}. {}{optional}", i + 1, stage.kind_name());
        }
        if let Some(est) = forza::state::estimate_cost(&template.name, &sd) {
            println!(
                "Estimated cost: ${:.2} - ${:.2} (avg ${:.2}, based on {} previous {} runs)",
                est.min, est.max, est.avg, est.count, est.workflow
            );
        }
        return ExitCode::SUCCESS;
    }

    let cli_overrides = forza::config::CliOverrides {
        model: args.model,
        skills: args.skill,
    };
    match forza::orchestrator::process_issue_with_overrides(
        args.number,
        &repo,
        routes,
        config,
        &sd,
        &rd,
        cli_overrides,
    )
    .await
    {
        Ok(record) => print_run_result(&record),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn cmd_pr(args: PrArgs, config: &forza::RunnerConfig) -> ExitCode {
    let sd = state_dir();

    let (repo, rd, routes) = match resolve_repo(args.repo.as_deref(), &args.repo_dir, config).await
    {
        Ok(r) => r,
        Err(code) => return code,
    };

    if args.dry_run {
        let pr = match forza::github::fetch_pr(&repo, args.number).await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        };

        let (route_name, route) = match forza::RunnerConfig::match_pr_route_in(routes, &pr) {
            Some(r) => r,
            None => {
                eprintln!(
                    "no route matches PR #{} (labels: {:?})",
                    pr.number, pr.labels
                );
                return ExitCode::FAILURE;
            }
        };

        let wf_name = route.workflow.as_deref().unwrap_or("");
        let template = match config.resolve_workflow(wf_name) {
            Some(t) => t,
            None => {
                eprintln!("unknown workflow: {wf_name}");
                return ExitCode::FAILURE;
            }
        };

        let branch = forza::RunnerConfig::branch_for_pr(&pr);
        let run_id = forza::state::generate_run_id();
        let plan = forza::planner::create_pr_plan(&pr, &template, &branch, &run_id);

        println!("PR:       #{} — {}", pr.number, pr.title);
        println!("Route:    {route_name}");
        println!("Workflow: {}", template.name);
        println!("Branch:   {branch}");
        let effective_model = args
            .model
            .as_deref()
            .or_else(|| config.effective_model(route));
        if let Some(model) = effective_model {
            println!("Model:    {model}");
        }
        println!("Stages:");
        for (i, stage) in plan.stages.iter().enumerate() {
            let optional = if stage.optional { " (optional)" } else { "" };
            println!("  {}. {}{optional}", i + 1, stage.kind_name());
        }
        return ExitCode::SUCCESS;
    }

    let cli_overrides = forza::config::CliOverrides {
        model: args.model,
        skills: args.skill,
    };
    match forza::orchestrator::process_pr_with_overrides(
        args.number,
        &repo,
        routes,
        config,
        &sd,
        &rd,
        cli_overrides,
    )
    .await
    {
        Ok(record) => print_run_result(&record),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn cmd_run(args: RunArgs, config: &forza::RunnerConfig) -> ExitCode {
    let sd = state_dir();

    // Resolve a local directory for every configured repo before doing any work.
    let mut repos_resolved: Vec<(
        String,
        PathBuf,
        std::collections::HashMap<String, forza::config::Route>,
    )> = Vec::new();
    for (repo, entry_repo_dir, routes) in config.iter_repos() {
        let explicit_dir = entry_repo_dir
            .map(PathBuf::from)
            .or_else(|| args.repo_dir.clone())
            .or_else(|| config.global.repo_dir.as_ref().map(PathBuf::from));
        let rd = match forza::isolation::find_or_clone_repo(repo, explicit_dir).await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        };
        repos_resolved.push((repo.to_string(), rd, routes.clone()));
    }

    let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

    let mut all_records = Vec::new();
    for (repo, rd, routes) in &repos_resolved {
        if *cancel_rx.borrow() {
            break;
        }
        let mut records =
            forza::orchestrator::process_batch_for_repo(repo, config, &sd, rd, routes, &cancel_rx)
                .await;
        all_records.append(&mut records);
    }
    let records = all_records;

    println!();
    let succeeded = records
        .iter()
        .filter(|r| r.status == forza::state::RunStatus::Succeeded)
        .count();
    let failed = records
        .iter()
        .filter(|r| r.status == forza::state::RunStatus::Failed)
        .count();
    println!(
        "Processed {} issues: {succeeded} succeeded, {failed} failed",
        records.len()
    );

    if records
        .iter()
        .all(|r| r.status == forza::state::RunStatus::Succeeded)
    {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

async fn cmd_watch(args: WatchArgs, config: &forza::RunnerConfig) -> ExitCode {
    let sd = state_dir();

    // Use the override interval or the default of 60 seconds.
    let interval = args.interval.unwrap_or(60);
    let interval_dur = std::time::Duration::from_secs(interval);

    let mut repos_data: Vec<(
        String,
        PathBuf,
        std::collections::HashMap<String, forza::config::Route>,
    )> = Vec::new();
    for (repo, entry_repo_dir, routes) in config.iter_repos() {
        let explicit_dir = entry_repo_dir
            .map(PathBuf::from)
            .or_else(|| args.repo_dir.clone())
            .or_else(|| config.global.repo_dir.as_ref().map(PathBuf::from));
        let rd = match forza::isolation::find_or_clone_repo(repo, explicit_dir).await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        };
        repos_data.push((repo.to_string(), rd, routes.clone()));
    }

    info!(
        repos = repos_data.len(),
        interval_secs = interval,
        "starting watch mode"
    );

    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

    // SIGTERM handler (Unix only).
    #[cfg(unix)]
    {
        let tx = cancel_tx.clone();
        tokio::spawn(async move {
            use tokio::signal::unix::{SignalKind, signal};
            if let Ok(mut sigterm) = signal(SignalKind::terminate()) {
                sigterm.recv().await;
                let _ = tx.send(true);
            }
        });
    }

    // SIGINT / Ctrl-C handler (all platforms).
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        let _ = cancel_tx.send(true);
    });

    // Spawn one independent poll-loop task per repo.
    let mut handles = Vec::new();
    for (repo, rd, routes) in repos_data {
        let config_clone = config.clone();
        let sd_clone = sd.clone();
        let cancel_rx_clone = cancel_rx.clone();
        handles.push(tokio::spawn(async move {
            info!(repo = repo, "starting repo watch loop");
            loop {
                let records = forza::orchestrator::process_batch_for_repo(
                    &repo,
                    &config_clone,
                    &sd_clone,
                    &rd,
                    &routes,
                    &cancel_rx_clone,
                )
                .await;
                if !records.is_empty() {
                    let succeeded = records
                        .iter()
                        .filter(|r| r.status == forza::state::RunStatus::Succeeded)
                        .count();
                    info!(
                        repo = repo,
                        processed = records.len(),
                        succeeded = succeeded,
                        "batch complete"
                    );
                }

                let removed = forza::isolation::cleanup_stale_worktrees(
                    &rd,
                    ".worktrees",
                    config_clone.global.stale_worktree_days,
                    false,
                )
                .await;
                for path in &removed {
                    info!(repo = repo, worktree = %path.display(), "removed stale worktree");
                }

                if *cancel_rx_clone.borrow() {
                    break;
                }

                let mut cancel_rx_sleep = cancel_rx_clone.clone();
                tokio::select! {
                    _ = tokio::time::sleep(interval_dur) => {},
                    _ = cancel_rx_sleep.changed() => break,
                }
            }
        }));
    }

    for handle in handles {
        handle.await.ok();
    }

    info!("watch mode stopped, exiting cleanly");
    ExitCode::SUCCESS
}

fn cmd_status(args: StatusArgs) -> ExitCode {
    let sd = state_dir();

    if args.all {
        let records = forza::state::load_all_runs(&sd);
        if records.is_empty() {
            eprintln!("no runs found");
            return ExitCode::FAILURE;
        }
        println!(
            "{:<30}  {:>6}  {:<20}  {:<10}  {:>8}  started_at",
            "run_id", "issue#", "workflow", "status", "cost"
        );
        println!("{}", "-".repeat(100));
        for r in &records {
            let cost = r
                .total_cost_usd
                .map(|c| format!("${c:.2}"))
                .unwrap_or_else(|| "-".into());
            println!(
                "{:<30}  {:>6}  {:<20}  {:<10}  {:>8}  {}",
                r.run_id,
                r.issue_number,
                r.workflow,
                r.status_text(),
                cost,
                r.started_at.format("%Y-%m-%d %H:%M:%S"),
            );
        }
        return ExitCode::SUCCESS;
    }

    if args.summary {
        let summaries = forza::state::summarize_by_workflow(&sd);
        if summaries.is_empty() {
            eprintln!("no runs found");
            return ExitCode::FAILURE;
        }
        println!(
            "{:<20}  {:>6}  {:>9}  {:>6}  {:>8}  {:>8}  {:>8}",
            "workflow", "total", "succeeded", "failed", "min $", "max $", "avg $"
        );
        println!("{}", "-".repeat(80));
        for s in &summaries {
            let fmt = |v: Option<f64>| v.map(|x| format!("${x:.2}")).unwrap_or_else(|| "-".into());
            println!(
                "{:<20}  {:>6}  {:>9}  {:>6}  {:>8}  {:>8}  {:>8}",
                s.workflow,
                s.total_runs,
                s.succeeded,
                s.failed,
                fmt(s.min_cost),
                fmt(s.max_cost),
                fmt(s.avg_cost),
            );
        }
        return ExitCode::SUCCESS;
    }

    if let Some(ref run_id) = args.run_id {
        match forza::state::load_run(run_id, &sd) {
            Some(record) => {
                println!("{}", serde_json::to_string_pretty(&record).unwrap());
                ExitCode::SUCCESS
            }
            None => {
                eprintln!("run not found: {run_id}");
                ExitCode::FAILURE
            }
        }
    } else {
        match forza::state::load_latest(&sd) {
            Some(record) => print_run_result(&record),
            None => {
                eprintln!("no runs found");
                ExitCode::FAILURE
            }
        }
    }
}

async fn cmd_fix(args: FixArgs, config: &forza::RunnerConfig) -> ExitCode {
    let sd = state_dir();

    // Find the run to fix.
    let record = if let Some(ref run_id) = args.run {
        forza::state::load_run(run_id, &sd)
    } else if let Some(issue_number) = args.issue {
        forza::state::find_latest_run_for_issue(issue_number, &sd)
    } else {
        forza::state::load_latest(&sd)
    };

    let record = match record {
        Some(r) => r,
        None => {
            eprintln!("error: no run found to fix");
            return ExitCode::FAILURE;
        }
    };

    if record.status == forza::state::RunStatus::Succeeded {
        eprintln!("run {} already succeeded, nothing to fix", record.run_id);
        return ExitCode::SUCCESS;
    }

    // Find the first failed stage.
    let failed_stage = record
        .stages
        .iter()
        .find(|s| s.status == forza::state::StageStatus::Failed);

    let failed_stage = match failed_stage {
        Some(s) => s,
        None => {
            eprintln!("no failed stages found in run {}", record.run_id);
            return ExitCode::SUCCESS;
        }
    };

    let error_context = failed_stage
        .result
        .as_ref()
        .map(|r| r.output.clone())
        .unwrap_or_default();

    println!(
        "Fixing run {} (issue #{})",
        record.run_id, record.issue_number
    );
    println!("Failed stage: {}", failed_stage.kind_name());
    if !error_context.is_empty() {
        println!("Error: {}", &error_context[..error_context.len().min(200)]);
    }
    println!();

    // Resolve the repo and routes from the run record.
    let repo = record.repo.clone();
    let (routes, explicit_dir) = if let Some(entry) = config.repos.get(&repo) {
        (&entry.routes, entry.repo_dir.as_ref().map(PathBuf::from))
    } else {
        (
            &config.routes,
            config.global.repo_dir.as_ref().map(PathBuf::from),
        )
    };
    let rd = match forza::isolation::find_or_clone_repo(&repo, explicit_dir).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Re-run the issue with error context injected.
    // The worktree should still exist (we keep them on failure).
    match forza::orchestrator::process_issue_with_config(
        record.issue_number,
        &repo,
        routes,
        config,
        &sd,
        &rd,
    )
    .await
    {
        Ok(new_record) => print_run_result(&new_record),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn cmd_clean(args: CleanArgs, config: &forza::RunnerConfig) -> ExitCode {
    let sd = state_dir();

    if args.runs {
        let files = forza::state::list_run_files(&sd);
        if files.is_empty() {
            println!("no run files found");
            return ExitCode::SUCCESS;
        }
        for path in &files {
            println!("{}", path.display());
        }
        if args.dry_run {
            println!("dry run: {} file(s) would be removed", files.len());
        } else {
            for path in &files {
                if let Err(e) = std::fs::remove_file(path) {
                    eprintln!("error removing {}: {e}", path.display());
                    return ExitCode::FAILURE;
                }
            }
            println!("removed {} file(s)", files.len());
        }
    } else if args.stale {
        let days = args.days.unwrap_or(config.global.stale_worktree_days);
        for (repo, entry_repo_dir, _routes) in config.iter_repos() {
            let explicit_dir = entry_repo_dir
                .map(PathBuf::from)
                .or_else(|| args.repo_dir.clone())
                .or_else(|| config.global.repo_dir.as_ref().map(PathBuf::from));
            let rd = match forza::isolation::find_or_clone_repo(repo, explicit_dir).await {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let removed =
                forza::isolation::cleanup_stale_worktrees(&rd, ".worktrees", days, args.dry_run)
                    .await;
            if removed.is_empty() {
                println!("no stale worktrees found in {}", rd.display());
            } else {
                for wt in &removed {
                    println!("{}", wt.display());
                }
                if args.dry_run {
                    println!(
                        "dry run: {} stale worktree(s) would be removed",
                        removed.len()
                    );
                } else {
                    println!("removed {} stale worktree(s)", removed.len());
                }
            }
        }
    } else {
        for (repo, entry_repo_dir, _routes) in config.iter_repos() {
            let explicit_dir = entry_repo_dir
                .map(PathBuf::from)
                .or_else(|| args.repo_dir.clone())
                .or_else(|| config.global.repo_dir.as_ref().map(PathBuf::from));
            let rd = match forza::isolation::find_or_clone_repo(repo, explicit_dir).await {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let worktrees = forza::isolation::list_worktrees(&rd, ".worktrees");
            if worktrees.is_empty() {
                println!("no worktrees found in {}", rd.display());
                continue;
            }
            for wt in &worktrees {
                println!("{}", wt.display());
            }
            if args.dry_run {
                println!("dry run: {} worktree(s) would be removed", worktrees.len());
            } else {
                for wt in &worktrees {
                    if let Err(e) = forza::isolation::remove_worktree(&rd, wt, true).await {
                        eprintln!("error removing worktree {}: {e}", wt.display());
                        return ExitCode::FAILURE;
                    }
                }
                println!("removed {} worktree(s)", worktrees.len());
            }
        }
    }

    ExitCode::SUCCESS
}

async fn cmd_serve(args: ServeArgs, config: forza::RunnerConfig) -> ExitCode {
    use std::sync::Arc;

    let sd = state_dir();
    let state = Arc::new(forza::api::AppState {
        config,
        state_dir: sd,
    });

    let addr: std::net::SocketAddr = match format!("{}:{}", args.host, args.port).parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: invalid address {}:{}: {e}", args.host, args.port);
            return ExitCode::FAILURE;
        }
    };

    let router = forza::api::router(state);

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: could not bind to {addr}: {e}");
            return ExitCode::FAILURE;
        }
    };

    info!(address = %addr, "REST API server listening");

    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

    // SIGTERM handler (Unix only).
    #[cfg(unix)]
    {
        let tx = cancel_tx.clone();
        tokio::spawn(async move {
            use tokio::signal::unix::{SignalKind, signal};
            if let Ok(mut sigterm) = signal(SignalKind::terminate()) {
                sigterm.recv().await;
                let _ = tx.send(true);
            }
        });
    }

    // SIGINT / Ctrl-C handler (all platforms).
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        let _ = cancel_tx.send(true);
    });

    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let mut rx = cancel_rx;
            // Wait until the channel signals true.
            loop {
                if rx.changed().await.is_err() || *rx.borrow() {
                    break;
                }
            }
        })
        .await
        .ok();

    info!("REST API server stopped");
    ExitCode::SUCCESS
}

async fn cmd_mcp(config: &forza::RunnerConfig) -> ExitCode {
    let sd = state_dir();
    let state = forza::mcp::AppState::new(config.clone(), sd);
    let router = forza::mcp::build_router(state);
    if let Err(e) = tower_mcp::StdioTransport::new(router).run().await {
        eprintln!("mcp server error: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn print_run_result(record: &forza::state::RunRecord) -> ExitCode {
    let subject = match record.subject_kind {
        forza::state::SubjectKind::Issue => format!("issue #{}", record.issue_number),
        forza::state::SubjectKind::Pr => format!("PR #{}", record.issue_number),
    };
    println!();
    println!(
        "Run {} — {} ({})",
        record.run_id,
        record.status_text(),
        subject
    );
    for stage in &record.stages {
        let cost = stage
            .result
            .as_ref()
            .and_then(|r| r.cost_usd)
            .map(|c| format!("  ${c:.2}"))
            .unwrap_or_default();
        let duration = stage
            .result
            .as_ref()
            .map(|r| format!("  {:.0}s", r.duration_secs))
            .unwrap_or_default();
        println!(
            "  {:<15} {:?}{duration}{cost}",
            stage.kind_name(),
            stage.status
        );
    }
    if let Some(pr) = record.pr_number {
        println!("PR: #{pr}");
    }
    if let Some(cost) = record.total_cost_usd {
        println!("Total cost: ${cost:.2}");
    }
    if record.status == forza::state::RunStatus::Succeeded {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
