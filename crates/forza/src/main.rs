use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use forza::plan::{
    build_issue_refs, build_issue_summaries, build_route_summary, parse_plan_dag,
    topological_levels,
};
use tracing::info;

/// Autonomous GitHub issue runner — turns issues into pull requests.
#[derive(Debug, Parser)]
#[command(
    name = "forza",
    version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("GIT_HASH"), ")"),
    about,
    long_about = "Autonomous GitHub issue runner that processes issues through\n\
        configurable workflow templates (plan -> implement -> test -> PR).\n\n\
        Agent-agnostic: uses Claude by default, pluggable for other agents."
)]
struct Cli {
    /// Path to config file. When omitted, forza.toml is used if present;
    /// for `issue` and `pr` commands the config is optional.
    #[arg(long, short, global = true)]
    config: Option<PathBuf>,

    /// Write tracing output to this file instead of stderr.
    #[arg(long, global = true)]
    log_file: Option<PathBuf>,

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
    /// Run once — poll for eligible issues and process them. Use --watch for continuous mode.
    Run(RunArgs),
    /// Watch mode — continuously poll and process issues (deprecated: use `run --watch`).
    #[command(hide = true)]
    Watch(WatchArgs),
    /// Show run history and status.
    Status(StatusArgs),
    /// Re-run failed stages with error context (deprecated: use `issue --fix` or `pr --fix`).
    #[command(hide = true)]
    Fix(FixArgs),
    /// Remove worktrees or run state.
    Clean(CleanArgs),
    /// Serve the REST API.
    Serve(ServeArgs),
    /// Start the MCP server (stdio or HTTP/SSE transport).
    Mcp(McpArgs),
    /// Show a structured breakdown of the loaded config and route paths.
    Explain(ExplainArgs),
    /// Open a new GitHub issue using agent assistance.
    Open(OpenArgs),
    /// Create or execute a plan: analyze issues, build a dependency graph, coordinate work.
    Plan(PlanArgs),
}

#[derive(Debug, Parser)]
#[command(
    after_long_help = "Examples:\n  forza init --repo owner/name\n  forza init --repo owner/name --output ci.toml\n  forza init --repo owner/name --auto\n  forza init --repo owner/name --auto --model claude-opus-4-6\n  forza init --repo owner/name --guided"
)]
struct InitArgs {
    /// Repository in owner/name format (e.g. acme/myrepo).
    #[arg(long)]
    repo: String,
    /// Output path for the generated config file.
    #[arg(long, default_value = "forza.toml")]
    output: std::path::PathBuf,
    /// Use an agent to inspect the repo and generate a tailored config.
    #[arg(long)]
    auto: bool,
    /// Launch an interactive Claude session to collaboratively generate a config.
    #[arg(long)]
    guided: bool,
    /// Model to use for agent-assisted config generation (e.g. claude-opus-4-6).
    #[arg(long)]
    model: Option<String>,
}

#[derive(Debug, Parser)]
#[command(
    after_long_help = "Examples:\n  forza fix\n  forza fix --issue 42\n  forza fix --run <run-id>"
)]
struct FixArgs {
    /// Run ID to fix (default: latest run).
    #[arg(long)]
    run: Option<String>,
    /// Issue number to fix (finds latest run for this issue).
    #[arg(long)]
    issue: Option<u64>,
}

#[derive(Debug, Parser)]
#[command(
    after_long_help = "Examples:\n  forza issue 42\n  forza issue 42 --dry-run --model claude-opus-4-6\n  forza issue 42 --skill ./skills/extra.md\n  forza issue 42 --workflow feature\n  forza issue 42 --fix"
)]
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
    /// Override the workflow template, skipping route matching (e.g. feature, bug, chore).
    #[arg(long)]
    workflow: Option<String>,
    /// Re-run the latest failed run for this issue.
    #[arg(long)]
    fix: bool,
}

#[derive(Debug, Parser)]
#[command(
    after_long_help = "Examples:\n  forza pr 123\n  forza pr 123 --route fix-pr\n  forza pr 123 --dry-run\n  forza pr 123 --workflow pr-fix\n  forza pr 123 --fix"
)]
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
    /// Force a specific route by name, bypassing label-based matching.
    #[arg(long)]
    route: Option<String>,
    /// Override the workflow template, skipping route matching (e.g. pr-fix, pr-rebase).
    #[arg(long)]
    workflow: Option<String>,
    /// Re-run the latest failed run for this PR.
    #[arg(long)]
    fix: bool,
}

#[derive(Debug, Parser)]
#[command(
    after_long_help = "Examples:\n  forza run\n  forza run --repo-dir . --no-gate\n  forza run --route bugfix\n  forza run --watch\n  forza run --watch --interval 30 --serve-api"
)]
struct RunArgs {
    /// Repository directory (default: current directory).
    #[arg(long)]
    repo_dir: Option<PathBuf>,
    /// Only run a specific route.
    #[arg(long)]
    route: Option<String>,
    /// Bypass the gate_label requirement and process all matching issues immediately.
    #[arg(long, default_value = "false")]
    no_gate: bool,
    /// Watch mode — continuously poll and process issues.
    #[arg(long, default_value = "false")]
    watch: bool,
    /// Override poll interval in seconds (watch mode only).
    #[arg(long)]
    interval: Option<u64>,
    /// Also start the REST API server alongside the watch loop (watch mode only).
    #[arg(long, default_value = "false")]
    serve_api: bool,
    /// Host address for the REST API server (watch mode only, default: 127.0.0.1).
    #[arg(long)]
    api_host: Option<String>,
    /// Port for the REST API server (watch mode only, default: 8080).
    #[arg(long)]
    api_port: Option<u16>,
}

#[derive(Debug, Parser)]
#[command(
    after_long_help = "Examples:\n  forza watch\n  forza watch --repo-dir . --interval 30 --serve-api\n  forza watch --route bugfix"
)]
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
    /// Also start the REST API server alongside the watch loop.
    #[arg(long, default_value = "false")]
    serve_api: bool,
    /// Host address for the REST API server (default: 127.0.0.1).
    #[arg(long)]
    api_host: Option<String>,
    /// Port for the REST API server (default: 8080).
    #[arg(long)]
    api_port: Option<u16>,
    /// Bypass the gate_label requirement and process all matching issues immediately.
    #[arg(long, default_value = "false")]
    no_gate: bool,
}

#[derive(Debug, Parser)]
#[command(
    after_long_help = "Examples:\n  forza status\n  forza status --all\n  forza status --run-id <id>\n  forza status --detailed"
)]
struct StatusArgs {
    /// Show a specific run by ID.
    #[arg(long)]
    run_id: Option<String>,
    /// Show all runs as a history table (sorted newest first).
    #[arg(long)]
    all: bool,
    /// Show latest run detail (old default behavior).
    #[arg(long)]
    detailed: bool,
    /// Filter dashboard to a single workflow.
    #[arg(long)]
    workflow: Option<String>,
}

#[derive(Debug, Parser)]
#[command(after_long_help = "Examples:\n  forza mcp\n  forza mcp --http --port 9090")]
struct McpArgs {
    /// Use HTTP/SSE transport instead of stdio.
    #[arg(long)]
    http: bool,
    /// Host address to bind to (HTTP mode only).
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    /// Port to listen on (HTTP mode only).
    #[arg(long, default_value_t = 8080)]
    port: u16,
}

#[derive(Debug, Parser)]
#[command(
    after_long_help = "Examples:\n  forza clean\n  forza clean --stale --days 7\n  forza clean --runs --dry-run"
)]
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
#[command(
    after_long_help = "Examples:\n  forza serve\n  forza serve --port 9090\n  forza serve --host 0.0.0.0 --port 9090"
)]
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

#[derive(Debug, Parser)]
#[command(
    after_long_help = "Examples:\n  forza explain\n  forza explain --issues\n  forza explain --route bugfix\n  forza explain --workflows\n  forza explain --json"
)]
struct ExplainArgs {
    /// Filter output to a single repository (owner/name).
    #[arg(long)]
    repo: Option<String>,
    /// Show only issue routes.
    #[arg(long)]
    issues: bool,
    /// Show only PR routes (label and condition).
    #[arg(long)]
    prs: bool,
    /// Show only condition routes.
    #[arg(long)]
    conditions: bool,
    /// Show a single route in detail (auto-verbose).
    #[arg(long)]
    route: Option<String>,
    /// List all workflow templates.
    #[arg(long)]
    workflows: bool,
    /// Show a single workflow's stages.
    #[arg(long)]
    workflow: Option<String>,
    /// Verbose output — show per-stage detail.
    #[arg(short, long)]
    verbose: bool,
    /// Output as JSON instead of human-readable text.
    #[arg(long)]
    json: bool,
    /// Show open plan issues and their execution status.
    #[arg(long)]
    plans: bool,
}

#[derive(Debug, Parser)]
#[command(
    after_long_help = "Examples:\n  forza open --repo owner/name\n  forza open --repo owner/name --prompt \"add retry backoff\"\n  forza open --repo owner/name --label enhancement --ready"
)]
struct OpenArgs {
    /// Repository to open an issue in (owner/name). Required when multiple repos are configured.
    #[arg(long)]
    repo: Option<String>,
    /// Prompt describing the issue to open.
    #[arg(long)]
    prompt: Option<String>,
    /// Label to apply to the created issue.
    #[arg(long)]
    label: Option<String>,
    /// Also add the forza:ready label to the created issue.
    #[arg(long, default_value = "false")]
    ready: bool,
    /// Override the model (e.g. claude-opus-4-6).
    #[arg(long)]
    model: Option<String>,
}

#[derive(Debug, Parser)]
#[command(
    after_long_help = "Examples:\n  forza plan\n  forza plan 42\n  forza plan 10 20 30\n  forza plan 10..20\n  forza plan --label backlog\n  forza plan --revise 99\n  forza plan --exec 99\n  forza plan --exec 99 --dry-run\n  forza plan --exec 99 --close"
)]
struct PlanArgs {
    /// Issue numbers to plan. Supports single (42), multiple (10 20 30), range (10..20).
    /// If omitted, plans all open issues.
    #[arg(value_name = "ISSUES")]
    issues: Vec<String>,
    /// Only plan issues with this label.
    #[arg(long)]
    label: Option<String>,
    /// Repository (owner/name). Required when multiple repos configured.
    #[arg(long)]
    repo: Option<String>,
    /// Revise an existing plan issue based on new comments.
    #[arg(long, value_name = "PLAN_ISSUE", conflicts_with = "exec")]
    revise: Option<u64>,
    /// Execute an existing plan issue: process actionable items in dependency order.
    #[arg(long, value_name = "PLAN_ISSUE", conflicts_with = "revise")]
    exec: Option<u64>,
    /// Preview execution order without processing (use with --exec).
    #[arg(long)]
    dry_run: bool,
    /// Close the plan issue after all items are executed (use with --exec).
    #[arg(long)]
    close: bool,
    /// Override the model (e.g. claude-opus-4-6).
    #[arg(long)]
    model: Option<String>,
    /// Maximum number of issues to fetch when no specific issues are given.
    #[arg(long, default_value = "50")]
    limit: usize,
    /// Create and target a plan branch for all PRs (e.g. `plan/my-feature`).
    /// The branch is created from `origin/main` before execution begins.
    #[arg(long, value_name = "BRANCH")]
    branch: Option<String>,
}

fn state_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".forza")
        .join("runs")
}

/// Build the log filter. Respects RUST_LOG if set, otherwise uses a default
/// that suppresses noisy HTTP/TLS/connection-pool crates at debug level.
fn default_log_filter() -> tracing_subscriber::EnvFilter {
    tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(
            "info,hyper_util=warn,rustls=warn,tower=warn,octocrab=warn,claude_wrapper=info",
        )
    })
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

/// Resolve the config to use.
///
/// - Explicit `--config PATH`: load it (error if missing).
/// - No flag, `forza.toml` exists: load it.
/// - No flag, `forza.toml` absent, command is `issue`/`pr`: return a default config.
/// - No flag, `forza.toml` absent, other commands: error with a hint.
fn resolve_config(
    config_flag: &Option<PathBuf>,
    command: &Command,
) -> Result<forza::RunnerConfig, ExitCode> {
    if let Some(path) = config_flag {
        return load_config(path);
    }
    let default_path = PathBuf::from("forza.toml");
    if default_path.exists() {
        return load_config(&default_path);
    }
    if matches!(command, Command::Issue(_) | Command::Pr(_)) {
        return Ok(forza::RunnerConfig::default());
    }
    eprintln!("error: forza.toml not found");
    eprintln!(
        "hint: run `forza init --repo owner/name` to create a config, or pass --config <path>"
    );
    Err(ExitCode::FAILURE)
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    let _guard = if let Some(path) = &cli.log_file {
        let file_appender = tracing_appender::rolling::never(
            path.parent().unwrap_or_else(|| std::path::Path::new(".")),
            path.file_name().unwrap_or_default(),
        );
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
        tracing_subscriber::fmt()
            .with_env_filter(default_log_filter())
            .with_writer(non_blocking)
            .init();
        Some(guard)
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(default_log_filter())
            .init();
        None
    };

    // Create gh client early for init (before config is loaded).
    let gh_init: std::sync::Arc<dyn forza::github::GitHubClient> =
        std::sync::Arc::new(forza::github::GhCliClient::new());

    if let Command::Init(args) = cli.command {
        return cmd_init(args, &*gh_init).await;
    }

    let config = match resolve_config(&cli.config, &cli.command) {
        Ok(c) => c,
        Err(code) => return code,
    };

    // Construct the GitHub client based on config.
    let gh: std::sync::Arc<dyn forza::github::GitHubClient> = if config.global.github_backend
        == "gh-cli"
    {
        std::sync::Arc::new(forza::github::GhCliClient::new())
    } else {
        match forza::github::OctocrabClient::new().await {
            Ok(client) => std::sync::Arc::new(client),
            Err(e) => {
                eprintln!("error: failed to create octocrab client: {e}");
                eprintln!(
                    "hint: set GITHUB_TOKEN or run `gh auth login`, or use github_backend = \"gh-cli\" in forza.toml"
                );
                return ExitCode::FAILURE;
            }
        }
    };

    // Construct the git client based on config.
    let git: std::sync::Arc<dyn forza::git::GitClient> = if config.global.git_backend == "git-cli" {
        std::sync::Arc::new(forza::git::GitCliClient::new())
    } else {
        std::sync::Arc::new(forza::git::GixClient::new())
    };

    if !matches!(
        cli.command,
        Command::Status(_)
            | Command::Clean(_)
            | Command::Serve(_)
            | Command::Mcp(_)
            | Command::Explain(_)
    ) && let Err(e) = forza::deps::validate_dependencies(&config.global.agent, &*git).await
    {
        eprintln!("error: {e}");
        return ExitCode::FAILURE;
    }

    match cli.command {
        Command::Init(_) => unreachable!(),
        Command::Issue(args) => cmd_issue(args, &config, &gh, &git).await,
        Command::Pr(args) => cmd_pr(args, &config, &gh, &git).await,
        Command::Run(args) => cmd_run(args, &config, gh, git).await,
        Command::Watch(args) => cmd_watch(args, &config, gh, git).await,
        Command::Status(args) => cmd_status(args),
        Command::Fix(args) => cmd_fix(args, &config, &gh, &git).await,
        Command::Clean(args) => cmd_clean(args, &config, &git).await,
        Command::Serve(args) => cmd_serve(args, config, gh, git).await,
        Command::Mcp(args) => cmd_mcp(args, &config, gh, git).await,
        Command::Explain(args) => cmd_explain(args, &config, &gh).await,
        Command::Open(args) => cmd_open(args, &config, &git).await,
        Command::Plan(args) => cmd_plan(args, &config, &gh, &git).await,
    }
}

async fn cmd_open(
    args: OpenArgs,
    config: &forza::RunnerConfig,
    git: &std::sync::Arc<dyn forza::git::GitClient>,
) -> ExitCode {
    let (repo, rd, _routes) = match resolve_repo(args.repo.as_deref(), &None, config, &**git).await
    {
        Ok(r) => r,
        Err(code) => return code,
    };

    // Build preamble: base agent preamble + optional prompt + label hints.
    let mut preamble = forza_core::planner::make_preamble(&repo);
    if let Some(ref prompt) = args.prompt {
        preamble.push_str("\n\n");
        preamble.push_str(prompt);
    }
    if let Some(ref label) = args.label {
        preamble.push_str(&format!("\n\nApply the label `{label}` to the issue."));
    }
    if args.ready {
        preamble.push_str("\n\nAlso add the label `forza:ready` to the issue.");
    }

    let prompt = forza_core::planner::PROMPT_OPEN_ISSUE
        .replace("{preamble}", &preamble)
        .replace("{repo}", &repo);

    let agent: std::sync::Arc<dyn forza_core::AgentExecutor> = match config.global.agent.as_str() {
        "codex" => std::sync::Arc::new(forza::adapters::CodexAgentAdapter),
        _ => std::sync::Arc::new(forza::adapters::ClaudeAgentAdapter),
    };

    let model = args.model.as_deref().or(config.global.model.as_deref());

    match agent
        .execute("open", &prompt, &rd, model, &[], None, None, &[])
        .await
    {
        Ok(result) => {
            if !result.output.is_empty() {
                println!("{}", result.output);
            }
            if result.success {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn cmd_plan(
    args: PlanArgs,
    config: &forza::RunnerConfig,
    gh: &std::sync::Arc<dyn forza::github::GitHubClient>,
    git: &std::sync::Arc<dyn forza::git::GitClient>,
) -> ExitCode {
    let (repo, rd, routes) = match resolve_repo(args.repo.as_deref(), &None, config, &**git).await {
        Ok(r) => r,
        Err(code) => return code,
    };

    // Exec mode: execute an existing plan issue.
    if let Some(plan_number) = args.exec {
        return cmd_plan_exec(
            plan_number,
            &repo,
            &rd,
            config,
            gh,
            git,
            args.dry_run,
            args.close,
            args.branch,
        )
        .await;
    }

    // Revise mode: update an existing plan issue.
    if let Some(plan_number) = args.revise {
        return cmd_plan_revise(plan_number, &repo, &rd, config, gh).await;
    }

    // Fetch issues based on selection.
    let issues = match fetch_plan_issues(&args, gh, &repo).await {
        Ok(issues) => issues,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    if issues.is_empty() {
        println!("No issues to plan.");
        return ExitCode::SUCCESS;
    }

    println!("Planning {} issue(s) in {repo}...", issues.len());

    // Build route summary and issue summaries for the prompt.
    let route_summary = build_route_summary(routes);
    let issue_summaries = build_issue_summaries(&issues);
    let issue_refs = build_issue_refs(&issues);

    // Build prompt from template.
    let preamble = forza_core::planner::make_preamble(&repo);
    let prompt = forza_core::planner::PROMPT_CMD_PLAN
        .replace("{preamble}", &preamble)
        .replace("{repo}", &repo)
        .replace("{routes}", &route_summary)
        .replace("{issues}", &issue_summaries)
        .replace("{issue_refs}", &issue_refs);

    let allowed_tools: Vec<String> = vec![
        "Read".into(),
        "Glob".into(),
        "Grep".into(),
        "Bash(gh *)".into(),
    ];

    let agent: std::sync::Arc<dyn forza_core::AgentExecutor> = match config.global.agent.as_str() {
        "codex" => std::sync::Arc::new(forza::adapters::CodexAgentAdapter),
        _ => std::sync::Arc::new(forza::adapters::ClaudeAgentAdapter),
    };

    let model = args.model.as_deref().or(config.global.model.as_deref());

    match agent
        .execute("plan", &prompt, &rd, model, &[], None, None, &allowed_tools)
        .await
    {
        Ok(result) => {
            if !result.output.is_empty() {
                println!("{}", result.output);
            }
            if result.success {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Execute an existing plan issue: process actionable items in dependency order.
#[allow(clippy::too_many_arguments)]
async fn cmd_plan_exec(
    plan_number: u64,
    repo: &str,
    rd: &std::path::Path,
    config: &forza::RunnerConfig,
    gh: &std::sync::Arc<dyn forza::github::GitHubClient>,
    git: &std::sync::Arc<dyn forza::git::GitClient>,
    dry_run: bool,
    close: bool,
    branch_override: Option<String>,
) -> ExitCode {
    let plan_issue = match gh.fetch_issue(repo, plan_number).await {
        Ok(i) => i,
        Err(e) => {
            eprintln!("error fetching plan issue #{plan_number}: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Verify this is a plan issue.
    if !plan_issue.labels.iter().any(|l| l == "forza:plan") {
        eprintln!("error: issue #{plan_number} is not a plan issue (missing forza:plan label)");
        return ExitCode::FAILURE;
    }

    // Parse the mermaid dependency graph from the plan body.
    let dag = match parse_plan_dag(&plan_issue.body) {
        Ok(dag) => dag,
        Err(e) => {
            eprintln!("error parsing plan: {e}");
            return ExitCode::FAILURE;
        }
    };

    if dag.is_empty() {
        println!("No actionable issues found in plan #{plan_number}.");
        return ExitCode::SUCCESS;
    }

    // Group issues into parallel execution levels.
    let levels = match topological_levels(&dag) {
        Ok(levels) => levels,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let total_issues: usize = levels.iter().map(|l| l.len()).sum();

    let sd = state_dir();
    let routes = match resolve_repo(None, &None, config, &**git).await {
        Ok((_, _, routes)) => routes.clone(),
        Err(code) => return code,
    };

    // Create the plan branch from origin/main if requested.
    if !dry_run
        && let Some(ref branch) = branch_override
        && let Err(e) = git.create_branch_from(rd, branch, "origin/main").await
    {
        eprintln!("error: failed to create plan branch '{branch}': {e}");
        return ExitCode::FAILURE;
    }

    if dry_run {
        println!(
            "Plan #{plan_number}: {total_issues} issues across {} level(s)\n",
            levels.len()
        );
        for (level_idx, level) in levels.iter().enumerate() {
            println!("Level {} ({} concurrent):", level_idx + 1, level.len());
            for issue_number in level {
                let deps = dag.get(issue_number).cloned().unwrap_or_default();
                let dep_str = if deps.is_empty() {
                    "(no dependencies)".to_string()
                } else {
                    let refs: Vec<String> = deps.iter().map(|d| format!("#{d}")).collect();
                    format!("depends on {}", refs.join(", "))
                };
                println!("  #{issue_number} -- {dep_str}");
            }
        }
        return ExitCode::SUCCESS;
    }

    println!(
        "Executing plan #{plan_number}: {total_issues} issues across {} level(s)",
        levels.len()
    );

    let mut succeeded = 0;
    let mut failed = 0;
    let mut skipped: std::collections::HashSet<u64> = std::collections::HashSet::new();
    // Track PR numbers created by each issue for merge-gating.
    let mut prs: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();

    let mut plan_exec = forza::state::PlanExecRecord {
        plan_number,
        repo: repo.to_string(),
        started_at: chrono::Utc::now(),
        issues: Vec::new(),
    };

    let max_concurrency = config.global.max_concurrency;

    'levels: for (level_idx, level) in levels.iter().enumerate() {
        // Skip issues in this level whose dependencies failed.
        let runnable: Vec<u64> = level
            .iter()
            .filter(|issue_number| {
                if let Some(deps) = dag.get(issue_number)
                    && deps.iter().any(|d| skipped.contains(d))
                {
                    println!("  Skipping #{issue_number} (dependency failed or skipped)");
                    false
                } else {
                    true
                }
            })
            .copied()
            .collect();

        // Mark non-runnable issues as skipped.
        for issue_number in level {
            if !runnable.contains(issue_number) {
                skipped.insert(*issue_number);
                plan_exec.issues.push(forza::state::PlanIssueEntry {
                    issue_number: *issue_number,
                    status: forza::state::PlanIssueStatus::Skipped,
                    pr_number: None,
                    pr_merged: false,
                    failed_stage: None,
                });
            }
        }
        let _ = forza::state::save_plan_exec(&plan_exec, &sd);

        if runnable.is_empty() {
            continue;
        }

        // Gate on prerequisite PR merges from previous levels before starting this level.
        for issue_number in &runnable {
            if let Some(deps) = dag.get(issue_number) {
                for dep in deps {
                    if let Some(&pr_number) = prs.get(dep) {
                        match wait_for_pr_merge(repo, pr_number, config.global.auto_merge, gh).await
                        {
                            MergeWaitResult::Merged => {
                                println!("    PR #{pr_number} (from #{dep}) merged, continuing");
                            }
                            MergeWaitResult::NeedsHumanMerge => {
                                println!(
                                    "\n  Paused: PR #{pr_number} (from #{dep}) needs to be merged before level {} can start.",
                                    level_idx + 1
                                );
                                println!(
                                    "  Merge the PR, then re-run: forza plan --exec {plan_number}"
                                );
                                println!(
                                    "\nPlan #{plan_number} paused: {succeeded} succeeded, {failed} failed, waiting on PR #{pr_number}",
                                );
                                return ExitCode::SUCCESS;
                            }
                            MergeWaitResult::Failed => {
                                eprintln!(
                                    "    PR #{pr_number} (from #{dep}) closed without merging"
                                );
                                skipped.insert(*issue_number);
                                failed += 1;
                                continue 'levels;
                            }
                        }
                    }
                }
            }
        }

        println!(
            "  Level {}: processing {} issue(s) concurrently",
            level_idx + 1,
            runnable.len()
        );

        // Process issues in this level concurrently, capped at max_concurrency.
        let mut join_set: tokio::task::JoinSet<(u64, forza_core::Result<forza_core::Run>)> =
            tokio::task::JoinSet::new();

        for chunk in runnable.chunks(max_concurrency) {
            for &issue_number in chunk {
                // Skip if already completed (resume support).
                match gh.fetch_issue(repo, issue_number).await {
                    Ok(issue) if issue.labels.iter().any(|l| l == "forza:complete") => {
                        println!("    #{issue_number}: already complete, skipping");
                        succeeded += 1;
                        plan_exec.issues.push(forza::state::PlanIssueEntry {
                            issue_number,
                            status: forza::state::PlanIssueStatus::Succeeded,
                            pr_number: None,
                            pr_merged: false,
                            failed_stage: None,
                        });
                        let _ = forza::state::save_plan_exec(&plan_exec, &sd);
                        continue;
                    }
                    _ => {}
                }

                let config_clone = config.clone();
                let routes_clone = routes.clone();
                let sd_clone = sd.clone();
                let rd_owned = rd.to_path_buf();
                let gh_clone = gh.clone();
                let git_clone = git.clone();
                let repo_owned = repo.to_string();
                let branch_override_clone = branch_override.clone();

                join_set.spawn(async move {
                    let result = forza::runner::process_issue(
                        issue_number,
                        &repo_owned,
                        &config_clone,
                        &routes_clone,
                        &sd_clone,
                        &rd_owned,
                        gh_clone,
                        git_clone,
                        None,
                        vec![],
                        branch_override_clone,
                        None,
                    )
                    .await;
                    (issue_number, result)
                });
            }

            while let Some(join_result) = join_set.join_next().await {
                match join_result {
                    Ok((issue_number, Ok(run))) => match run.status {
                        forza_core::RunStatus::Succeeded => {
                            succeeded += 1;
                            let (pr_number, pr_merged) = match run.outcome.as_ref() {
                                Some(forza_core::Outcome::PrCreated { number }) => {
                                    (Some(*number), false)
                                }
                                Some(forza_core::Outcome::PrMerged { number }) => {
                                    (Some(*number), true)
                                }
                                _ => (None, false),
                            };
                            if let Some(forza_core::Outcome::PrCreated { number })
                            | Some(forza_core::Outcome::PrMerged { number }) = run.outcome
                            {
                                prs.insert(issue_number, number);
                            }
                            plan_exec.issues.push(forza::state::PlanIssueEntry {
                                issue_number,
                                status: forza::state::PlanIssueStatus::Succeeded,
                                pr_number,
                                pr_merged,
                                failed_stage: None,
                            });
                            let _ = forza::state::save_plan_exec(&plan_exec, &sd);
                            println!("    #{issue_number}: succeeded");
                        }
                        _ => {
                            failed += 1;
                            skipped.insert(issue_number);
                            let failed_stage =
                                run.failed_stage().map(|s| s.kind_name().to_string());
                            plan_exec.issues.push(forza::state::PlanIssueEntry {
                                issue_number,
                                status: forza::state::PlanIssueStatus::Failed,
                                pr_number: None,
                                pr_merged: false,
                                failed_stage,
                            });
                            let _ = forza::state::save_plan_exec(&plan_exec, &sd);
                            println!("    #{issue_number}: failed");
                        }
                    },
                    Ok((issue_number, Err(e))) => {
                        eprintln!("    #{issue_number}: error: {e}");
                        failed += 1;
                        skipped.insert(issue_number);
                        plan_exec.issues.push(forza::state::PlanIssueEntry {
                            issue_number,
                            status: forza::state::PlanIssueStatus::Failed,
                            pr_number: None,
                            pr_merged: false,
                            failed_stage: None,
                        });
                        let _ = forza::state::save_plan_exec(&plan_exec, &sd);
                    }
                    Err(e) => {
                        eprintln!("    task join error: {e}");
                    }
                }
            }
        }
    }

    println!(
        "\nPlan #{plan_number} complete: {succeeded} succeeded, {failed} failed, {} skipped",
        skipped.len().saturating_sub(failed)
    );

    if let Some(ref branch) = branch_override {
        println!("Plan branch ready: {branch}");
    }

    if close {
        let summary = format!(
            "Plan execution complete: {succeeded} succeeded, {failed} failed, {} skipped.",
            skipped.len().saturating_sub(failed)
        );
        if let Err(e) = gh.comment_on_issue(repo, plan_number, &summary).await {
            eprintln!("warning: failed to post summary comment on #{plan_number}: {e}");
        }
        if let Err(e) = gh.close_issue(repo, plan_number).await {
            eprintln!("warning: failed to close plan issue #{plan_number}: {e}");
        } else {
            println!("Closed plan issue #{plan_number}.");
        }
    }

    if failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

enum MergeWaitResult {
    Merged,
    NeedsHumanMerge,
    Failed,
}

/// Wait for a PR to be merged (if auto-merge is enabled) or report that it needs merging.
async fn wait_for_pr_merge(
    repo: &str,
    pr_number: u64,
    auto_merge: bool,
    gh: &std::sync::Arc<dyn forza::github::GitHubClient>,
) -> MergeWaitResult {
    // Check current state first.
    match gh.fetch_pr(repo, pr_number).await {
        Ok(pr) => {
            if pr.state == "MERGED" {
                return MergeWaitResult::Merged;
            }
            if pr.state == "CLOSED" {
                return MergeWaitResult::Failed;
            }
        }
        Err(_) => return MergeWaitResult::Failed,
    }

    if !auto_merge {
        return MergeWaitResult::NeedsHumanMerge;
    }

    // Poll until merged (auto-merge enabled).
    println!("    Waiting for PR #{pr_number} to be merged (auto-merge)...");
    let poll_interval = std::time::Duration::from_secs(30);
    let max_wait = std::time::Duration::from_secs(3600); // 1 hour max
    let start = std::time::Instant::now();

    loop {
        tokio::time::sleep(poll_interval).await;

        match gh.fetch_pr(repo, pr_number).await {
            Ok(pr) => {
                if pr.state == "MERGED" {
                    return MergeWaitResult::Merged;
                }
                if pr.state == "CLOSED" {
                    return MergeWaitResult::Failed;
                }
            }
            Err(e) => {
                tracing::warn!(pr = pr_number, error = %e, "error checking PR state");
            }
        }

        if start.elapsed() > max_wait {
            tracing::warn!(pr = pr_number, "timed out waiting for PR merge");
            return MergeWaitResult::NeedsHumanMerge;
        }
    }
}

/// Revise an existing plan issue based on new comments.
async fn cmd_plan_revise(
    plan_number: u64,
    repo: &str,
    rd: &std::path::Path,
    config: &forza::RunnerConfig,
    gh: &std::sync::Arc<dyn forza::github::GitHubClient>,
) -> ExitCode {
    let plan_issue = match gh.fetch_issue(repo, plan_number).await {
        Ok(i) => i,
        Err(e) => {
            eprintln!("error fetching plan issue #{plan_number}: {e}");
            return ExitCode::FAILURE;
        }
    };

    println!("Revising plan issue #{plan_number} in {repo}...");

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

    let preamble = forza_core::planner::make_preamble(repo);
    let prompt = forza_core::planner::PROMPT_CMD_PLAN_REVISE
        .replace("{preamble}", &preamble)
        .replace("{repo}", repo)
        .replace("{plan_number}", &plan_number.to_string())
        .replace("{plan_body}", &plan_issue.body)
        .replace("{comments}", &comments_text);

    let allowed_tools: Vec<String> = vec![
        "Read".into(),
        "Glob".into(),
        "Grep".into(),
        "Bash(gh *)".into(),
    ];

    let agent: std::sync::Arc<dyn forza_core::AgentExecutor> = match config.global.agent.as_str() {
        "codex" => std::sync::Arc::new(forza::adapters::CodexAgentAdapter),
        _ => std::sync::Arc::new(forza::adapters::ClaudeAgentAdapter),
    };

    let model = config.global.model.as_deref();

    match agent
        .execute("plan", &prompt, rd, model, &[], None, None, &allowed_tools)
        .await
    {
        Ok(result) => {
            if !result.output.is_empty() {
                println!("{}", result.output);
            }
            if result.success {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Parse issue selectors and fetch issues from GitHub.
async fn fetch_plan_issues(
    args: &PlanArgs,
    gh: &std::sync::Arc<dyn forza::github::GitHubClient>,
    repo: &str,
) -> forza::error::Result<Vec<forza::github::IssueCandidate>> {
    // If a label filter is specified, use it (apply limit client-side).
    if let Some(ref label) = args.label {
        let mut issues = gh.fetch_issues_with_label(repo, label).await?;
        issues.truncate(args.limit);
        return Ok(issues);
    }

    // If specific issues are given, parse and fetch them.
    if !args.issues.is_empty() {
        let numbers = parse_issue_numbers(&args.issues)?;
        let mut issues = Vec::with_capacity(numbers.len());
        for num in numbers {
            issues.push(gh.fetch_issue(repo, num).await?);
        }
        return Ok(issues);
    }

    // Default: fetch all open issues (no label filter).
    let issues = gh.fetch_eligible_issues(repo, &[], args.limit).await?;

    // Filter out issues with forza lifecycle labels.
    let lifecycle = ["forza:in-progress", "forza:complete", "forza:needs-human"];
    Ok(issues
        .into_iter()
        .filter(|i| !i.labels.iter().any(|l| lifecycle.contains(&l.as_str())))
        .collect())
}

/// Parse issue number arguments: single numbers (42) and ranges (10..20).
fn parse_issue_numbers(args: &[String]) -> forza::error::Result<Vec<u64>> {
    let mut numbers = Vec::new();
    for arg in args {
        if let Some((start, end)) = arg.split_once("..") {
            let start: u64 = start.parse().map_err(|_| {
                forza::error::Error::Triage(format!("invalid range start: {start}"))
            })?;
            let end: u64 = end
                .parse()
                .map_err(|_| forza::error::Error::Triage(format!("invalid range end: {end}")))?;
            if start > end {
                return Err(forza::error::Error::Triage(format!(
                    "invalid range: {start}..{end} (start > end)"
                )));
            }
            numbers.extend(start..=end);
        } else {
            let num: u64 = arg
                .parse()
                .map_err(|_| forza::error::Error::Triage(format!("invalid issue number: {arg}")))?;
            numbers.push(num);
        }
    }
    Ok(numbers)
}

async fn cmd_init(args: InitArgs, gh: &dyn forza::github::GitHubClient) -> ExitCode {
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
        ("forza:plan", "5319e7", "Forza plan issue"),
    ];

    println!("Creating forza labels in {}...", args.repo);
    for (name, color, description) in labels {
        match gh.create_label(&args.repo, name, color, description).await {
            Ok(()) => println!("  label: {name}"),
            Err(e) => {
                eprintln!("error creating label {name}: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    if args.guided {
        use claude_wrapper::Claude;

        let prompt_template = include_str!("prompts/init_guided.md");
        let system_prompt = prompt_template
            .replace("{repo}", &args.repo)
            .replace("{output}", &args.output.display().to_string());

        let claude = match Claude::builder().working_dir(".").build() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("error creating claude client: {e}");
                return ExitCode::FAILURE;
            }
        };

        let mut cmd = tokio::process::Command::new(claude.binary());
        cmd.arg("--append-system-prompt")
            .arg(&system_prompt)
            .arg("--allowedTools")
            .arg("Read,Glob,Grep,Write,Bash(gh *)")
            .current_dir(".")
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit());

        if let Some(ref model) = args.model {
            cmd.arg("--model").arg(model);
        }

        println!("Starting guided config session for {}...", args.repo);
        let status = match cmd.status().await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("failed to launch claude: {e}");
                return ExitCode::FAILURE;
            }
        };

        if !status.success() {
            eprintln!("guided session exited with status: {status}");
            return ExitCode::FAILURE;
        }

        // Validate the generated config.
        match std::fs::read_to_string(&args.output) {
            Ok(contents) => match toml::from_str::<forza::RunnerConfig>(&contents) {
                Ok(_) => {}
                Err(e) => {
                    eprintln!(
                        "generated config at {} is invalid: {e}",
                        args.output.display()
                    );
                    return ExitCode::FAILURE;
                }
            },
            Err(e) => {
                eprintln!(
                    "agent did not write config to {}: {e}",
                    args.output.display()
                );
                return ExitCode::FAILURE;
            }
        }

        println!("Created {}", args.output.display());
        return ExitCode::SUCCESS;
    }

    if args.auto {
        use claude_wrapper::streaming::{StreamEvent, stream_query};
        use claude_wrapper::{Claude, OutputFormat, PermissionMode, QueryCommand};

        let prompt_template = include_str!("prompts/init_auto.md");
        let prompt = prompt_template
            .replace("{repo}", &args.repo)
            .replace("{output}", &args.output.display().to_string());

        let claude = match Claude::builder().working_dir(".").build() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("error creating claude client: {e}");
                return ExitCode::FAILURE;
            }
        };

        let mut cmd = QueryCommand::new(&prompt)
            .output_format(OutputFormat::StreamJson)
            .permission_mode(PermissionMode::BypassPermissions)
            .no_session_persistence()
            .allowed_tools(["Read", "Glob", "Grep", "Write", "Bash(gh *)"]);

        if let Some(ref model) = args.model {
            cmd = cmd.model(model);
        }

        println!("Generating config with agent assistance...");
        let output = match stream_query(&claude, &cmd, |event: StreamEvent| {
            if let Some(t) = event.event_type() {
                info!(event_type = t, "init auto event");
            }
        })
        .await
        {
            Ok(o) => o,
            Err(e) => {
                eprintln!("agent execution failed: {e}");
                return ExitCode::FAILURE;
            }
        };

        if !output.success {
            eprintln!("agent did not complete successfully:\n{}", output.stderr);
            return ExitCode::FAILURE;
        }

        // Validate the generated config.
        match std::fs::read_to_string(&args.output) {
            Ok(contents) => match toml::from_str::<forza::RunnerConfig>(&contents) {
                Ok(_) => {}
                Err(e) => {
                    eprintln!(
                        "generated config at {} is invalid: {e}",
                        args.output.display()
                    );
                    return ExitCode::FAILURE;
                }
            },
            Err(e) => {
                eprintln!(
                    "agent did not write config to {}: {e}",
                    args.output.display()
                );
                return ExitCode::FAILURE;
            }
        }

        println!("Created {}", args.output.display());
        return ExitCode::SUCCESS;
    }

    // Detect language and set validation commands + context template.
    let (language, validation_commands) = if std::path::Path::new("Cargo.toml").exists() {
        (
            Some("rust"),
            "commands = [\n    \"cargo fmt --all -- --check\",\n    \"cargo clippy --all-targets -- -D warnings\",\n    \"cargo test --lib\",\n]",
        )
    } else if std::path::Path::new("package.json").exists() {
        (
            Some("node"),
            "commands = [\n    \"npm run lint\",\n    \"npm test\",\n]",
        )
    } else if std::path::Path::new("pyproject.toml").exists()
        || std::path::Path::new("setup.py").exists()
    {
        (Some("python"), "commands = [\n    \"python -m pytest\",\n]")
    } else if std::path::Path::new("go.mod").exists() {
        (
            Some("go"),
            "commands = [\n    \"go vet ./...\",\n    \"go test ./...\",\n]",
        )
    } else {
        (None, "# commands = []  # add your validation commands here")
    };

    // Write language context template if detected.
    let agent_config_block = if let Some(lang) = language {
        let context_dir = std::path::Path::new("forza-context");
        if let Err(e) = std::fs::create_dir_all(context_dir) {
            eprintln!("warning: could not create forza-context/: {e}");
        }

        let template_content: &str = match lang {
            "rust" => include_str!("context_templates/rust.md"),
            "node" => include_str!("context_templates/node.md"),
            "python" => include_str!("context_templates/python.md"),
            "go" => include_str!("context_templates/go.md"),
            _ => unreachable!(),
        };

        let context_path = context_dir.join(format!("{lang}.md"));
        match std::fs::write(&context_path, template_content) {
            Ok(()) => {
                println!("Created {}", context_path.display());
                format!("\n[agent_config]\ncontext = [\"./forza-context/{lang}.md\"]\n")
            }
            Err(e) => {
                eprintln!("warning: could not write {}: {e}", context_path.display());
                String::new()
            }
        }
    } else {
        String::new()
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
{agent_config}
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
        agent_config = agent_config_block,
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
/// Parse a GitHub `owner/name` slug from a remote URL.
///
/// Handles both HTTPS (`https://github.com/owner/name.git`) and SSH
/// (`git@github.com:owner/name.git`) formats.
fn slug_from_remote_url(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches(".git");
    // SSH: git@github.com:owner/name
    if let Some(rest) = url.split_once(':').map(|(_, r)| r)
        && rest.contains('/')
        && !rest.contains("//")
    {
        return Some(rest.to_string());
    }
    // HTTPS: https://github.com/owner/name
    let parts: Vec<&str> = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .splitn(3, '/')
        .collect();
    if parts.len() == 3 {
        return Some(format!("{}/{}", parts[1], parts[2]));
    }
    None
}

async fn resolve_repo<'a>(
    args_repo: Option<&str>,
    args_repo_dir: &Option<PathBuf>,
    config: &'a forza::RunnerConfig,
    git: &dyn forza::git::GitClient,
) -> Result<
    (
        String,
        PathBuf,
        &'a indexmap::IndexMap<String, forza::config::Route>,
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

    // When no repo is configured (default config), infer it from the git remote.
    let repo_slug = if repo_str.is_empty() {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let url = match git.remote_url(&cwd).await {
            Ok(u) => u,
            Err(e) => {
                eprintln!("error: no repo configured and could not read git remote: {e}");
                eprintln!("hint: run from a git checkout or set repo in forza.toml");
                return Err(ExitCode::FAILURE);
            }
        };
        match slug_from_remote_url(&url) {
            Some(s) => s,
            None => {
                eprintln!("error: could not parse owner/name from remote URL: {url}");
                eprintln!("hint: set repo = \"owner/name\" in forza.toml");
                return Err(ExitCode::FAILURE);
            }
        }
    } else {
        repo_str.to_string()
    };

    let rd = match forza::isolation::find_or_clone_repo(&repo_slug, explicit_dir, git).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return Err(ExitCode::FAILURE);
        }
    };

    Ok((repo_slug, rd, routes))
}

async fn cmd_issue(
    args: IssueArgs,
    config: &forza::RunnerConfig,
    gh: &std::sync::Arc<dyn forza::github::GitHubClient>,
    git: &std::sync::Arc<dyn forza::git::GitClient>,
) -> ExitCode {
    let sd = state_dir();

    let (repo, rd, routes) =
        match resolve_repo(args.repo.as_deref(), &args.repo_dir, config, &**git).await {
            Ok(r) => r,
            Err(code) => return code,
        };

    if args.dry_run {
        let issue = match gh.fetch_issue(&repo, args.number).await {
            Ok(i) => i,
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        };

        // Detect plan issues — auto-dispatch to plan --exec.
        if issue.labels.iter().any(|l| l == "forza:plan") {
            tracing::info!(
                "issue #{} is a plan issue; dispatching to plan --exec",
                issue.number
            );
            return cmd_plan_exec(issue.number, &repo, &rd, config, gh, git, true, false, None)
                .await;
        }

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
        let plan =
            forza::planner::create_plan_with_config(&issue, &template, &branch, None, &run_id);

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

    // Check for plan issues before processing — auto-dispatch to plan --exec.
    if let Ok(issue) = gh.fetch_issue(&repo, args.number).await
        && issue.labels.iter().any(|l| l == "forza:plan")
    {
        tracing::info!(
            "issue #{} is a plan issue; dispatching to plan --exec",
            args.number
        );
        return cmd_plan_exec(args.number, &repo, &rd, config, gh, git, false, false, None).await;
    }

    // --fix: find latest failed run and re-process.
    if args.fix {
        let record = forza::state::find_latest_run_for_issue(args.number, &sd)
            .filter(|r| r.status == forza::state::RunStatus::Failed);
        let record = match record {
            Some(r) => r,
            None => {
                eprintln!("error: no failed run found for issue #{}", args.number);
                return ExitCode::FAILURE;
            }
        };
        println!(
            "Fixing run {} (issue #{})",
            record.run_id, record.issue_number
        );
        match forza::runner::process_issue(
            args.number,
            &repo,
            config,
            routes,
            &sd,
            &rd,
            gh.clone(),
            git.clone(),
            args.model,
            args.skill,
            None,
            args.workflow,
        )
        .await
        {
            Ok(run) => return print_core_run(&run),
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    match forza::runner::process_issue(
        args.number,
        &repo,
        config,
        routes,
        &sd,
        &rd,
        gh.clone(),
        git.clone(),
        args.model,
        args.skill,
        None,
        args.workflow,
    )
    .await
    {
        Ok(run) => print_core_run(&run),
        Err(forza_core::Error::GitHub(msg)) => {
            eprintln!("error: {msg}");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn cmd_pr(
    args: PrArgs,
    config: &forza::RunnerConfig,
    gh: &std::sync::Arc<dyn forza::github::GitHubClient>,
    git: &std::sync::Arc<dyn forza::git::GitClient>,
) -> ExitCode {
    let sd = state_dir();

    let (repo, rd, routes) =
        match resolve_repo(args.repo.as_deref(), &args.repo_dir, config, &**git).await {
            Ok(r) => r,
            Err(code) => return code,
        };

    if args.dry_run {
        let pr = match gh.fetch_pr(&repo, args.number).await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        };

        let (route_name, route) = if let Some(ref rn) = args.route
            && let Some(r) = routes.get(rn)
        {
            (rn.as_str(), r)
        } else {
            match forza::RunnerConfig::match_pr_route_in(routes, &pr) {
                Some(r) => r,
                None => {
                    eprintln!(
                        "no route matches PR #{} (labels: {:?})",
                        pr.number, pr.labels
                    );
                    return ExitCode::FAILURE;
                }
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

    // --fix: find latest failed run for this PR and re-process.
    if args.fix {
        let record = forza::state::load_all_runs(&sd).into_iter().find(|r| {
            r.issue_number == args.number
                && r.subject_kind == forza::state::SubjectKind::Pr
                && r.status == forza::state::RunStatus::Failed
        });
        let record = match record {
            Some(r) => r,
            None => {
                eprintln!("error: no failed run found for PR #{}", args.number);
                return ExitCode::FAILURE;
            }
        };
        println!("Fixing run {} (PR #{})", record.run_id, record.issue_number);
        match forza::runner::process_pr(
            args.number,
            &repo,
            config,
            routes,
            &sd,
            &rd,
            gh.clone(),
            git.clone(),
            args.model,
            args.skill,
            args.route,
            args.workflow,
        )
        .await
        {
            Ok(run) => return print_core_run(&run),
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    match forza::runner::process_pr(
        args.number,
        &repo,
        config,
        routes,
        &sd,
        &rd,
        gh.clone(),
        git.clone(),
        args.model,
        args.skill,
        args.route,
        args.workflow,
    )
    .await
    {
        Ok(run) => print_core_run(&run),
        Err(forza_core::Error::GitHub(msg)) => {
            eprintln!("error: {msg}");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn cmd_run(
    args: RunArgs,
    config: &forza::RunnerConfig,
    gh: std::sync::Arc<dyn forza::github::GitHubClient>,
    git: std::sync::Arc<dyn forza::git::GitClient>,
) -> ExitCode {
    // Delegate to watch mode when --watch is set.
    if args.watch {
        let watch_args = WatchArgs {
            interval: args.interval,
            route: args.route,
            repo_dir: args.repo_dir,
            serve_api: args.serve_api,
            api_host: args.api_host,
            api_port: args.api_port,
            no_gate: args.no_gate,
        };
        return cmd_watch(watch_args, config, gh, git).await;
    }

    let mut config = config.clone();
    if args.no_gate {
        config.global.gate_label = None;
    }
    let config = &config;
    let sd = state_dir();

    // Resolve a local directory for every configured repo before doing any work.
    let mut repos_resolved: Vec<(
        String,
        PathBuf,
        indexmap::IndexMap<String, forza::config::Route>,
    )> = Vec::new();
    for (repo, entry_repo_dir, routes) in config.iter_repos() {
        let explicit_dir = entry_repo_dir
            .map(PathBuf::from)
            .or_else(|| args.repo_dir.clone())
            .or_else(|| config.global.repo_dir.as_ref().map(PathBuf::from));
        let rd = match forza::isolation::find_or_clone_repo(repo, explicit_dir, &*git).await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        };
        let routes = if let Some(ref route_filter) = args.route {
            routes
                .iter()
                .filter(|(name, _)| *name == route_filter)
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        } else {
            routes.clone()
        };
        repos_resolved.push((repo.to_string(), rd, routes));
    }

    let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

    let mut all_records = Vec::new();
    for (repo, rd, routes) in &repos_resolved {
        if *cancel_rx.borrow() {
            break;
        }
        let mut runs = forza::runner::process_batch(
            repo,
            config,
            &sd,
            rd,
            routes,
            &cancel_rx,
            gh.clone(),
            git.clone(),
        )
        .await;
        all_records.append(&mut runs);
    }
    let records = all_records;

    println!();
    let succeeded = records
        .iter()
        .filter(|r| r.status == forza_core::RunStatus::Succeeded)
        .count();
    let failed = records
        .iter()
        .filter(|r| r.status == forza_core::RunStatus::Failed)
        .count();
    println!(
        "Processed {} issues: {succeeded} succeeded, {failed} failed",
        records.len()
    );

    if records
        .iter()
        .all(|r| r.status == forza_core::RunStatus::Succeeded)
    {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

async fn cmd_watch(
    args: WatchArgs,
    config: &forza::RunnerConfig,
    gh: std::sync::Arc<dyn forza::github::GitHubClient>,
    git: std::sync::Arc<dyn forza::git::GitClient>,
) -> ExitCode {
    let mut config = config.clone();
    if args.no_gate {
        config.global.gate_label = None;
    }
    let config = &config;
    let sd = state_dir();

    // Use the override interval or the default of 60 seconds.
    let interval = args.interval.unwrap_or(60);
    let interval_dur = std::time::Duration::from_secs(interval);

    let mut repos_data: Vec<(
        String,
        PathBuf,
        indexmap::IndexMap<String, forza::config::Route>,
    )> = Vec::new();
    for (repo, entry_repo_dir, routes) in config.iter_repos() {
        let explicit_dir = entry_repo_dir
            .map(PathBuf::from)
            .or_else(|| args.repo_dir.clone())
            .or_else(|| config.global.repo_dir.as_ref().map(PathBuf::from));
        let rd = match forza::isolation::find_or_clone_repo(repo, explicit_dir, &*git).await {
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

    // Optionally spawn the REST API server alongside the poll loops.
    if args.serve_api {
        use std::sync::Arc;
        let host = args.api_host.as_deref().unwrap_or("127.0.0.1").to_string();
        let port = args.api_port.unwrap_or(8080);
        let addr: std::net::SocketAddr = match format!("{host}:{port}").parse() {
            Ok(a) => a,
            Err(e) => {
                eprintln!("error: invalid API address {host}:{port}: {e}");
                return ExitCode::FAILURE;
            }
        };
        let state = Arc::new(forza::api::AppState {
            config: config.clone(),
            state_dir: sd.clone(),
            gh: gh.clone(),
            git: git.clone(),
        });
        let router = forza::api::router(state);
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => {
                info!(address = %addr, "REST API server listening");
                let mut api_cancel_rx = cancel_rx.clone();
                tokio::spawn(async move {
                    axum::serve(listener, router)
                        .with_graceful_shutdown(async move {
                            loop {
                                if api_cancel_rx.changed().await.is_err() || *api_cancel_rx.borrow()
                                {
                                    break;
                                }
                            }
                        })
                        .await
                        .ok();
                    info!("REST API server stopped");
                });
            }
            Err(e) => {
                eprintln!("error: could not bind API server to {addr}: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    // Spawn one independent poll-loop task per repo.
    let mut handles = Vec::new();
    for (repo, rd, routes) in repos_data {
        let config_clone = config.clone();
        let sd_clone = sd.clone();
        let cancel_rx_clone = cancel_rx.clone();
        let gh_clone = gh.clone();
        let git_clone = git.clone();
        handles.push(tokio::spawn(async move {
            info!(repo = repo, "starting repo watch loop");
            loop {
                let records = forza::runner::process_batch(
                    &repo,
                    &config_clone,
                    &sd_clone,
                    &rd,
                    &routes,
                    &cancel_rx_clone,
                    gh_clone.clone(),
                    git_clone.clone(),
                )
                .await;
                if !records.is_empty() {
                    let succeeded = records
                        .iter()
                        .filter(|r| r.status == forza_core::RunStatus::Succeeded)
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
                    &*git_clone,
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

async fn cmd_explain(
    args: ExplainArgs,
    config: &forza::RunnerConfig,
    gh: &std::sync::Arc<dyn forza::github::GitHubClient>,
) -> ExitCode {
    if args.json {
        match serde_json::to_string_pretty(config) {
            Ok(json) => {
                println!("{json}");
                return ExitCode::SUCCESS;
            }
            Err(e) => {
                eprintln!("error serializing config: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    // --workflows: list all workflow templates.
    if args.workflows {
        println!("Workflow Templates");
        println!("{}", "=".repeat(60));
        for wf in forza_core::Workflow::builtins() {
            let stages: Vec<&str> = wf.stages.iter().map(|s| s.kind.name()).collect();
            let wt = if wf.needs_worktree {
                ""
            } else {
                " (no worktree)"
            };
            println!("  {:<16} {}{wt}", wf.name, stages.join(" -> "));
        }
        if !config.workflow_templates.is_empty() {
            println!("\n  Custom:");
            for wf in &config.workflow_templates {
                let stages: Vec<&str> = wf.stages.iter().map(|s| s.kind.name()).collect();
                println!("  {:<16} {}", wf.name, stages.join(" -> "));
            }
        }
        return ExitCode::SUCCESS;
    }

    // --workflow <name>: show a single workflow's stages in detail.
    if let Some(ref wf_name) = args.workflow {
        return explain_workflow(wf_name, config);
    }

    // --plans: list open plan issues and their execution status.
    if args.plans {
        let repos = config.iter_repos();
        println!("Plan Issues");
        println!("{}", "=".repeat(60));
        let mut found_any = false;
        for (repo_slug, _repo_dir, _routes) in &repos {
            if let Some(ref filter) = args.repo
                && repo_slug != filter
            {
                continue;
            }
            let plan_issues = match gh.fetch_issues_with_label(repo_slug, "forza:plan").await {
                Ok(issues) => issues,
                Err(e) => {
                    eprintln!("error fetching plan issues for {repo_slug}: {e}");
                    return ExitCode::FAILURE;
                }
            };
            for plan_issue in &plan_issues {
                found_any = true;
                let dag = match parse_plan_dag(&plan_issue.body) {
                    Ok(d) => d,
                    Err(_) => {
                        println!(
                            "  #{:<4} {}  (no dependency graph)",
                            plan_issue.number, plan_issue.title
                        );
                        continue;
                    }
                };
                let total = dag.len();
                let mut complete = 0usize;
                let mut open_states: std::collections::HashMap<u64, bool> =
                    std::collections::HashMap::new();
                for &issue_num in dag.keys() {
                    match gh.fetch_issue(repo_slug, issue_num).await {
                        Ok(issue) => {
                            let is_closed = issue.state.to_uppercase() == "CLOSED";
                            open_states.insert(issue_num, !is_closed);
                            if is_closed {
                                complete += 1;
                            }
                        }
                        Err(_) => {
                            open_states.insert(issue_num, true);
                        }
                    }
                }
                if complete == total {
                    println!(
                        "  #{:<4} {}  {}/{} complete",
                        plan_issue.number, plan_issue.title, complete, total
                    );
                } else if complete == 0 {
                    println!(
                        "  #{:<4} {}  0/{} complete, not started",
                        plan_issue.number, plan_issue.title, total
                    );
                } else {
                    let mut blocked = 0usize;
                    let mut pending = 0usize;
                    for (&issue_num, &is_open) in &open_states {
                        if !is_open {
                            continue;
                        }
                        let deps = dag.get(&issue_num).map(|v| v.as_slice()).unwrap_or(&[]);
                        let dep_open = deps
                            .iter()
                            .any(|d| open_states.get(d).copied().unwrap_or(false));
                        if dep_open {
                            blocked += 1;
                        } else {
                            pending += 1;
                        }
                    }
                    println!(
                        "  #{:<4} {}  {}/{} complete, {} blocked, {} pending",
                        plan_issue.number, plan_issue.title, complete, total, blocked, pending
                    );
                }
            }
        }
        if !found_any {
            println!("  (no open plan issues)");
        }
        return ExitCode::SUCCESS;
    }

    let repos = config.iter_repos();
    for (repo_slug, _repo_dir, routes) in &repos {
        if let Some(ref filter) = args.repo
            && repo_slug != filter
        {
            continue;
        }

        // Global header.
        println!("Repository: {repo_slug}");
        if let Some(ref gate) = config.global.gate_label {
            println!("  Gate label:     {gate}");
        }
        println!("  Agent:          {}", config.global.agent);
        println!("  Max concurrency: {}", config.global.max_concurrency);
        println!("  Security:       {}", config.security.authorization_level);
        let validation = &config.validation.commands;
        if !validation.is_empty() {
            println!("  Validation:     {} command(s)", validation.len());
        }

        // Single route mode (auto-verbose).
        if let Some(ref route_filter) = args.route {
            if let Some(route) = routes.get(route_filter) {
                println!();
                explain_route_verbose(route_filter, route, config);
            } else {
                eprintln!("route '{route_filter}' not found");
                return ExitCode::FAILURE;
            }
            return ExitCode::SUCCESS;
        }

        // Categorize routes.
        let issue_routes: Vec<_> = routes
            .iter()
            .filter(|(_, r)| r.route_type == forza::config::SubjectType::Issue)
            .collect();
        let pr_label_routes: Vec<_> = routes
            .iter()
            .filter(|(_, r)| r.route_type == forza::config::SubjectType::Pr && r.label.is_some())
            .collect();
        let pr_condition_routes: Vec<_> = routes
            .iter()
            .filter(|(_, r)| {
                r.route_type == forza::config::SubjectType::Pr && r.condition.is_some()
            })
            .collect();

        // Issue routes.
        if !issue_routes.is_empty() && !args.prs && !args.conditions {
            println!("\n-- Issue Routes {}", "-".repeat(45));
            for (name, route) in &issue_routes {
                explain_route_compact(name, route, config, args.verbose);
            }
        }

        // PR label routes.
        if !pr_label_routes.is_empty() && !args.issues && !args.conditions {
            println!("\n-- PR Routes (label) {}", "-".repeat(40));
            for (name, route) in &pr_label_routes {
                explain_route_compact(name, route, config, args.verbose);
            }
        }

        // PR condition routes.
        if !pr_condition_routes.is_empty() && !args.issues && (!args.prs || args.conditions) {
            println!("\n-- PR Routes (condition) {}", "-".repeat(37));
            for (name, route) in &pr_condition_routes {
                explain_route_compact(name, route, config, args.verbose);
            }
        }

        println!();
    }

    ExitCode::SUCCESS
}

fn explain_route_compact(
    name: &str,
    route: &forza::config::Route,
    config: &forza::RunnerConfig,
    verbose: bool,
) {
    if verbose {
        explain_route_verbose(name, route, config);
        return;
    }

    println!();
    println!("{name}");

    // Trigger.
    if let Some(ref label) = route.label {
        let gate = config
            .global
            .gate_label
            .as_deref()
            .map(|g| format!(" + \"{g}\""))
            .unwrap_or_default();
        println!("  Trigger:     label \"{label}\"{gate}");
    } else if let Some(ref cond) = route.condition {
        let scope = match route.scope {
            forza::config::ConditionScope::ForzaOwned => " (forza_owned)",
            forza::config::ConditionScope::All => " (all)",
        };
        println!("  Trigger:     {cond:?}{scope}");
    }

    // Workflow + stages.
    let wf_name = route.workflow.as_deref().unwrap_or("(none)");
    if let Some(wf) = forza::runner::resolve_workflow_public(config, wf_name) {
        let stages: Vec<String> = wf
            .stages
            .iter()
            .map(|s| {
                let name = s.kind.name();
                if s.optional {
                    format!("{name}*")
                } else {
                    name.to_string()
                }
            })
            .collect();
        println!("  Workflow:    {wf_name}");
        println!("  Stages:      {}", stages.join(" -> "));
    } else {
        println!("  Workflow:    {wf_name} (unresolved)");
    }

    // Concurrency + poll.
    println!("  Concurrency: {}", route.concurrency);
    if route.condition.is_some() {
        println!("  Poll:        {}s", route.poll_interval);
    }

    // Retries (condition routes only).
    if let Some(max) = route.max_retries {
        println!("  Retries:     {max} -> forza:needs-human");
    }
}

fn explain_route_verbose(name: &str, route: &forza::config::Route, config: &forza::RunnerConfig) {
    println!();
    println!("{name} (verbose)");

    // Trigger.
    if let Some(ref label) = route.label {
        let gate = config
            .global
            .gate_label
            .as_deref()
            .map(|g| format!(" + gate \"{g}\""))
            .unwrap_or_default();
        println!("  Trigger:     label \"{label}\"{gate}");
    } else if let Some(ref cond) = route.condition {
        let scope = match route.scope {
            forza::config::ConditionScope::ForzaOwned => " (forza_owned)",
            forza::config::ConditionScope::All => " (all)",
        };
        println!("  Trigger:     {cond:?}{scope}");
        println!("  Poll:        {}s", route.poll_interval);
    }

    // Model.
    let model = config
        .effective_model(route)
        .unwrap_or("(default)")
        .to_string();
    println!("  Model:       {model}");
    println!("  Agent:       {}", config.global.agent);

    // Skills.
    let skills = config.effective_skills(route, None);
    if skills.is_empty() {
        println!("  Skills:      (none)");
    } else {
        println!("  Skills:      {}", skills.join(", "));
    }

    // Workflow + per-stage detail.
    let wf_name = route.workflow.as_deref().unwrap_or("(none)");
    if let Some(wf) = forza::runner::resolve_workflow_public(config, wf_name) {
        println!("  Workflow:    {wf_name}");
        println!("  Stages:");
        for (i, stage) in wf.stages.iter().enumerate() {
            let exec = if stage.is_agentless() {
                "shell"
            } else {
                "agent"
            };
            let opt = if stage.optional { " (optional)" } else { "" };
            let cond = stage
                .condition
                .as_ref()
                .map(|c| format!(" [if: {c}]"))
                .unwrap_or_default();
            println!("    {}. {:<13} {exec}{opt}{cond}", i + 1, stage.kind.name());
        }
    }

    // Validation.
    let validation = config.effective_validation(route);
    if !validation.is_empty() {
        println!("  Validation:");
        for cmd in validation {
            println!("    - {cmd}");
        }
    }

    // Hooks.
    let hooks = &config.stage_hooks;
    if !hooks.is_empty() {
        let relevant: Vec<_> = hooks
            .iter()
            .filter(|(_, h)| !h.pre.is_empty() || !h.post.is_empty() || !h.finally.is_empty())
            .collect();
        if !relevant.is_empty() {
            println!("  Hooks:");
            for (stage, h) in &relevant {
                if !h.pre.is_empty() {
                    println!("    {stage}.pre:     {}", h.pre.join(", "));
                }
                if !h.post.is_empty() {
                    println!("    {stage}.post:    {}", h.post.join(", "));
                }
                if !h.finally.is_empty() {
                    println!("    {stage}.finally: {}", h.finally.join(", "));
                }
            }
        }
    }

    // Retries.
    if let Some(max) = route.max_retries {
        println!("  Retries:     {max} -> forza:needs-human");
    }

    println!("  On failure:  {} label", config.global.failed_label);
}

fn explain_workflow(name: &str, config: &forza::RunnerConfig) -> ExitCode {
    let wf = if let Some(wf) = forza::runner::resolve_workflow_public(config, name) {
        wf
    } else {
        eprintln!("workflow '{name}' not found");
        return ExitCode::FAILURE;
    };

    let wt = if wf.needs_worktree {
        "needs worktree"
    } else {
        "no worktree"
    };
    println!("Workflow: {} ({wt})", wf.name);
    println!("{}", "-".repeat(60));
    for (i, stage) in wf.stages.iter().enumerate() {
        let exec = if stage.is_agentless() {
            "shell"
        } else {
            "agent"
        };
        let opt = if stage.optional { " (optional)" } else { "" };
        let cond = stage
            .condition
            .as_ref()
            .map(|c| format!("\n     condition: {c}"))
            .unwrap_or_default();
        let cmd = stage
            .shell_command()
            .map(|c| {
                let truncated = if c.len() > 60 {
                    format!("{}...", &c[..60])
                } else {
                    c.to_string()
                };
                format!("\n     command: {truncated}")
            })
            .unwrap_or_default();
        println!(
            "  {}. {:<13} {exec}{opt}{cond}{cmd}",
            i + 1,
            stage.kind.name()
        );
    }
    ExitCode::SUCCESS
}

fn format_time_ago(dt: chrono::DateTime<chrono::Utc>) -> String {
    let secs = (chrono::Utc::now() - dt).num_seconds().max(0);
    if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}

fn print_status_dashboard(sd: &std::path::Path, workflow_filter: Option<&str>) -> ExitCode {
    let mut summaries = forza::state::summarize_by_workflow(sd);
    if let Some(filter) = workflow_filter {
        summaries.retain(|s| s.workflow == filter);
    }
    if summaries.is_empty() {
        eprintln!("no runs found");
        return ExitCode::FAILURE;
    }

    println!(
        "forza {}",
        concat!(env!("CARGO_PKG_VERSION"), " (", env!("GIT_HASH"), ")")
    );
    let sep = "─".repeat(53);
    println!(
        "{:<20}  {:>6}  {:>6}  {:>6}  {:>9}",
        "Workflows", "Runs", "Pass", "Fail", "Avg Cost"
    );
    println!("{sep}");

    let (mut total_runs, mut total_pass, mut total_fail) = (0usize, 0usize, 0usize);
    let mut total_cost_sum = 0f64;
    let mut total_cost_count = 0usize;

    for s in &summaries {
        let avg = s
            .avg_cost
            .map(|c| format!("${c:.2}"))
            .unwrap_or_else(|| "-".into());
        println!(
            "{:<20}  {:>6}  {:>6}  {:>6}  {:>9}",
            s.workflow, s.total_runs, s.succeeded, s.failed, avg
        );
        total_runs += s.total_runs;
        total_pass += s.succeeded;
        total_fail += s.failed;
        if let Some(avg_cost) = s.avg_cost {
            total_cost_sum += avg_cost * s.total_runs as f64;
            total_cost_count += s.total_runs;
        }
    }

    println!("{sep}");
    let total_avg = if total_cost_count > 0 {
        format!("${:.2}", total_cost_sum / total_cost_count as f64)
    } else {
        "-".into()
    };
    println!(
        "{:<20}  {:>6}  {:>6}  {:>6}  {:>9}",
        "Total", total_runs, total_pass, total_fail, total_avg
    );

    // Recent activity (last 24h)
    let cutoff = chrono::Utc::now() - chrono::Duration::hours(24);
    let all_runs = forza::state::load_all_runs(sd);
    let recent: Vec<_> = all_runs
        .iter()
        .filter(|r| r.started_at >= cutoff && workflow_filter.is_none_or(|f| r.workflow == f))
        .take(10)
        .collect();

    if !recent.is_empty() {
        println!();
        println!("Recent activity (last 24h):");
        for r in recent {
            use forza::state::RouteOutcome;
            let prefix = match r.outcome.as_ref() {
                Some(RouteOutcome::Failed { .. }) | Some(RouteOutcome::Exhausted { .. }) => "✗",
                Some(RouteOutcome::PrCreated { .. })
                | Some(RouteOutcome::PrUpdated { .. })
                | Some(RouteOutcome::PrMerged { .. })
                | Some(RouteOutcome::CommentPosted)
                | Some(RouteOutcome::NothingToDo) => "✓",
                None => "·",
            };
            let outcome = format_outcome(r.outcome.as_ref());
            let ago = format_time_ago(r.started_at);
            println!(
                "  {prefix} #{} {} → {}     {ago}",
                r.issue_number, r.workflow, outcome
            );
        }
    }

    // Plan Executions
    let plan_execs = forza::state::load_all_plan_execs(sd);
    if !plan_execs.is_empty() {
        use forza::state::PlanIssueStatus;
        println!();
        println!("Plan Executions:");
        for plan in &plan_execs {
            let n_succeeded = plan
                .issues
                .iter()
                .filter(|i| i.status == PlanIssueStatus::Succeeded)
                .count();
            let n_failed = plan
                .issues
                .iter()
                .filter(|i| i.status == PlanIssueStatus::Failed)
                .count();
            let n_skipped = plan
                .issues
                .iter()
                .filter(|i| i.status == PlanIssueStatus::Skipped)
                .count();
            let ago = format_time_ago(plan.started_at);
            println!(
                "  Plan #{} ({}) — {} issue(s): {}✓ {}✗ {}- ({ago})",
                plan.plan_number,
                plan.repo,
                plan.issues.len(),
                n_succeeded,
                n_failed,
                n_skipped,
            );
            for entry in &plan.issues {
                let (prefix, detail) = match entry.status {
                    PlanIssueStatus::Succeeded => {
                        let pr_info = match (entry.pr_number, entry.pr_merged) {
                            (Some(n), true) => format!("  PR #{n} (merged)"),
                            (Some(n), false) => format!("  PR #{n}"),
                            (None, _) => String::new(),
                        };
                        ("✓", format!("succeeded{pr_info}"))
                    }
                    PlanIssueStatus::Failed => {
                        let stage_info = entry
                            .failed_stage
                            .as_deref()
                            .map(|s| format!("  stage: {s}"))
                            .unwrap_or_default();
                        ("✗", format!("failed{stage_info}"))
                    }
                    PlanIssueStatus::Skipped => ("-", "skipped".to_string()),
                };
                println!("    {prefix} #{:<6}  {detail}", entry.issue_number);
            }
        }
    }

    ExitCode::SUCCESS
}

fn cmd_status(args: StatusArgs) -> ExitCode {
    let sd = state_dir();

    if let Some(ref run_id) = args.run_id {
        return match forza::state::load_run(run_id, &sd) {
            Some(record) => {
                println!("{}", serde_json::to_string_pretty(&record).unwrap());
                ExitCode::SUCCESS
            }
            None => {
                eprintln!("run not found: {run_id}");
                ExitCode::FAILURE
            }
        };
    }

    if args.all {
        let records = forza::state::load_all_runs(&sd);
        if records.is_empty() {
            eprintln!("no runs found");
            return ExitCode::FAILURE;
        }
        println!(
            "forza {}",
            concat!(env!("CARGO_PKG_VERSION"), " (", env!("GIT_HASH"), ")")
        );
        println!(
            "{:<30}  {:>6}  {:<20}  {:<10}  {:>8}  {:<25}  started_at",
            "run_id", "issue#", "workflow", "status", "cost", "outcome"
        );
        println!("{}", "-".repeat(127));
        for r in &records {
            let cost = r
                .total_cost_usd
                .map(|c| format!("${c:.2}"))
                .unwrap_or_else(|| "-".into());
            println!(
                "{:<30}  {:>6}  {:<20}  {:<10}  {:>8}  {:<25}  {}",
                r.run_id,
                r.issue_number,
                r.workflow,
                r.status_text(),
                cost,
                format_outcome(r.outcome.as_ref()),
                r.started_at.format("%Y-%m-%d %H:%M:%S"),
            );
        }
        return ExitCode::SUCCESS;
    }

    if args.detailed {
        return match forza::state::load_latest(&sd) {
            Some(record) => print_run_result(&record),
            None => {
                eprintln!("no runs found");
                ExitCode::FAILURE
            }
        };
    }

    print_status_dashboard(&sd, args.workflow.as_deref())
}

async fn cmd_fix(
    args: FixArgs,
    config: &forza::RunnerConfig,
    gh: &std::sync::Arc<dyn forza::github::GitHubClient>,
    git: &std::sync::Arc<dyn forza::git::GitClient>,
) -> ExitCode {
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
    let rd = match forza::isolation::find_or_clone_repo(&repo, explicit_dir, &**git).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Re-run the issue with error context injected.
    match forza::runner::process_issue(
        record.issue_number,
        &repo,
        config,
        routes,
        &sd,
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
        Ok(run) => print_core_run(&run),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn cmd_clean(
    args: CleanArgs,
    config: &forza::RunnerConfig,
    git: &std::sync::Arc<dyn forza::git::GitClient>,
) -> ExitCode {
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
            let rd = match forza::isolation::find_or_clone_repo(repo, explicit_dir, &**git).await {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let removed = forza::isolation::cleanup_stale_worktrees(
                &rd,
                ".worktrees",
                days,
                args.dry_run,
                &**git,
            )
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
            let rd = match forza::isolation::find_or_clone_repo(repo, explicit_dir, &**git).await {
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
                    if let Err(e) = forza::isolation::remove_worktree(&rd, wt, true, &**git).await {
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

async fn cmd_serve(
    args: ServeArgs,
    config: forza::RunnerConfig,
    gh: std::sync::Arc<dyn forza::github::GitHubClient>,
    git: std::sync::Arc<dyn forza::git::GitClient>,
) -> ExitCode {
    use std::sync::Arc;

    let sd = state_dir();
    let state = Arc::new(forza::api::AppState {
        config,
        state_dir: sd,
        gh,
        git,
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

async fn cmd_mcp(
    args: McpArgs,
    config: &forza::RunnerConfig,
    gh: std::sync::Arc<dyn forza::github::GitHubClient>,
    git: std::sync::Arc<dyn forza::git::GitClient>,
) -> ExitCode {
    let sd = state_dir();
    if args.http {
        info!(host = %args.host, port = args.port, "MCP HTTP/SSE server listening");
        if let Err(e) =
            forza::mcp::serve_http(config.clone(), sd, gh, git, &args.host, args.port).await
        {
            eprintln!("mcp server error: {e}");
            return ExitCode::FAILURE;
        }
    } else {
        let state = forza::mcp::AppState::new(config.clone(), sd, gh, git);
        let router = forza::mcp::build_router(state);
        if let Err(e) = tower_mcp::StdioTransport::new(router).run().await {
            eprintln!("mcp server error: {e}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

fn format_outcome(outcome: Option<&forza::state::RouteOutcome>) -> String {
    use forza::state::RouteOutcome;
    match outcome {
        None => "-".into(),
        Some(RouteOutcome::PrCreated { number }) => format!("pr_created (#{number})"),
        Some(RouteOutcome::PrUpdated { number }) => format!("pr_updated (#{number})"),
        Some(RouteOutcome::PrMerged { number }) => format!("pr_merged (#{number})"),
        Some(RouteOutcome::CommentPosted) => "comment_posted".into(),
        Some(RouteOutcome::NothingToDo) => "nothing_to_do".into(),
        Some(RouteOutcome::Failed { stage, .. }) => format!("failed (stage: {stage})"),
        Some(RouteOutcome::Exhausted { retries }) => format!("exhausted ({retries} retries)"),
    }
}

fn print_core_run(run: &forza_core::Run) -> ExitCode {
    let subject = match run.subject_kind {
        forza_core::SubjectKind::Issue => format!("issue #{}", run.subject_number),
        forza_core::SubjectKind::Pr => format!("PR #{}", run.subject_number),
    };
    println!();
    println!(
        "forza {} — Run {} — {} ({subject})",
        concat!(env!("CARGO_PKG_VERSION"), " (", env!("GIT_HASH"), ")"),
        run.run_id,
        run.status,
    );
    if let Some(ref outcome) = run.outcome {
        println!("  Outcome:  {outcome}");
    }
    for stage in &run.stages {
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
    if let Some(pr) = run.pr_number {
        println!("PR: #{pr}");
    }
    if let Some(cost) = run.total_cost_usd {
        println!("Total cost: ${cost:.2}");
    }
    if run.status == forza_core::RunStatus::Succeeded {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn print_run_result(record: &forza::state::RunRecord) -> ExitCode {
    let subject = match record.subject_kind {
        forza::state::SubjectKind::Issue => format!("issue #{}", record.issue_number),
        forza::state::SubjectKind::Pr => format!("PR #{}", record.issue_number),
    };
    println!();
    println!(
        "forza {} — Run {} — {} ({})",
        concat!(env!("CARGO_PKG_VERSION"), " (", env!("GIT_HASH"), ")"),
        record.run_id,
        record.status_text(),
        subject
    );
    println!("  Outcome:  {}", format_outcome(record.outcome.as_ref()));
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

#[cfg(test)]
mod tests {
    use super::*;
    use forza::plan::{compact_ranges, topological_sort};

    #[test]
    fn parse_plan_dag_extracts_edges_and_standalone_nodes() {
        let body = r##"
# Plan

## Dependency Graph

```mermaid
graph TD
    401["#401 CI workflow"] --> 403["#403 auto-fix-ci"]
    401 --> 404["#404 auto-merge"]
    401 --> 407["#407 scenario scripts"]
    402["#402 auto-rebase"]
    405["#405 concurrent issues"]
    406["#406 validation failure"]
```

## Actionable
"##;
        let dag = parse_plan_dag(body).unwrap();

        // 401 has no dependencies (it's a root).
        assert_eq!(dag.get(&401).unwrap(), &Vec::<u64>::new());

        // 403 depends on 401.
        assert_eq!(dag.get(&403).unwrap(), &vec![401]);

        // 404 depends on 401.
        assert_eq!(dag.get(&404).unwrap(), &vec![401]);

        // 407 depends on 401.
        assert_eq!(dag.get(&407).unwrap(), &vec![401]);

        // Standalone nodes.
        assert_eq!(dag.get(&402).unwrap(), &Vec::<u64>::new());
        assert_eq!(dag.get(&405).unwrap(), &Vec::<u64>::new());
        assert_eq!(dag.get(&406).unwrap(), &Vec::<u64>::new());

        assert_eq!(dag.len(), 7);
    }

    #[test]
    fn topological_sort_respects_dependencies() {
        let body = r##"
```mermaid
graph TD
    1["#1 base"] --> 2["#2 depends on 1"]
    1 --> 3["#3 depends on 1"]
    2 --> 4["#4 depends on 2"]
```
"##;
        let dag = parse_plan_dag(body).unwrap();
        let order = topological_sort(&dag).unwrap();

        let pos = |n: u64| order.iter().position(|&x| x == n).unwrap();

        // 1 must come before 2, 3, and 4.
        assert!(pos(1) < pos(2));
        assert!(pos(1) < pos(3));
        assert!(pos(1) < pos(4));

        // 2 must come before 4.
        assert!(pos(2) < pos(4));
    }

    #[test]
    fn topological_sort_standalone_nodes_sorted_by_number() {
        let body = r##"
```mermaid
graph TD
    30["#30 standalone"]
    10["#10 standalone"]
    20["#20 standalone"]
```
"##;
        let dag = parse_plan_dag(body).unwrap();
        let order = topological_sort(&dag).unwrap();

        // All standalone, should be sorted numerically.
        assert_eq!(order, vec![10, 20, 30]);
    }

    #[test]
    fn topological_sort_fan_out_deterministic() {
        // #69 is the root, #70-#76 all depend on #69.
        // After #69, the rest should appear in numeric order.
        let body = r##"
```mermaid
graph TD
    69["#69 root"] --> 75["#75 E"]
    69 --> 72["#72 B"]
    69 --> 76["#76 F"]
    69 --> 70["#70 A"]
    69 --> 73["#73 C"]
```
"##;
        let dag = parse_plan_dag(body).unwrap();
        let order = topological_sort(&dag).unwrap();

        assert_eq!(order, vec![69, 70, 72, 73, 75, 76]);
    }

    #[test]
    fn parse_plan_dag_no_mermaid_block() {
        let body = "# Plan\n\nNo graph here.";
        assert!(parse_plan_dag(body).is_err());
    }

    #[test]
    fn parse_issue_numbers_single() {
        assert_eq!(parse_issue_numbers(&["42".into()]).unwrap(), vec![42]);
    }

    #[test]
    fn parse_issue_numbers_range() {
        assert_eq!(
            parse_issue_numbers(&["10..13".into()]).unwrap(),
            vec![10, 11, 12, 13]
        );
    }

    #[test]
    fn parse_issue_numbers_mixed() {
        assert_eq!(
            parse_issue_numbers(&["5".into(), "10..12".into(), "20".into()]).unwrap(),
            vec![5, 10, 11, 12, 20]
        );
    }

    #[test]
    fn parse_issue_numbers_invalid() {
        assert!(parse_issue_numbers(&["abc".into()]).is_err());
    }

    #[test]
    fn parse_issue_numbers_reversed_range() {
        assert!(parse_issue_numbers(&["20..10".into()]).is_err());
    }

    #[test]
    fn compact_ranges_contiguous() {
        assert_eq!(compact_ranges(&[69, 70, 71, 72, 73]), "#69..#73");
    }

    #[test]
    fn compact_ranges_with_gaps() {
        assert_eq!(
            compact_ranges(&[69, 70, 71, 73, 75, 77, 78, 79]),
            "#69..#71, #73, #75, #77..#79"
        );
    }

    #[test]
    fn compact_ranges_all_singles() {
        assert_eq!(compact_ranges(&[10, 20, 30]), "#10, #20, #30");
    }

    #[test]
    fn compact_ranges_single_item() {
        assert_eq!(compact_ranges(&[42]), "#42");
    }
}
