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
    /// Start the MCP server (stdio or HTTP/SSE transport).
    Mcp(McpArgs),
    /// Show a structured breakdown of the loaded config and route paths.
    Explain(ExplainArgs),
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
    after_long_help = "Examples:\n  forza issue 42\n  forza issue 42 --dry-run --model claude-opus-4-6\n  forza issue 42 --skill ./skills/extra.md"
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
}

#[derive(Debug, Parser)]
#[command(
    after_long_help = "Examples:\n  forza pr 123\n  forza pr 123 --route fix-pr\n  forza pr 123 --dry-run"
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
}

#[derive(Debug, Parser)]
#[command(after_long_help = "Examples:\n  forza run\n  forza run --repo-dir . --no-gate")]
struct RunArgs {
    /// Repository directory.
    #[arg(long)]
    repo_dir: Option<PathBuf>,
    /// Bypass the gate_label requirement and process all matching issues immediately.
    #[arg(long, default_value = "false")]
    no_gate: bool,
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
    after_long_help = "Examples:\n  forza explain\n  forza explain --repo owner/name\n  forza explain --json"
)]
struct ExplainArgs {
    /// Filter output to a single repository (owner/name).
    #[arg(long)]
    repo: Option<String>,
    /// Output as JSON instead of human-readable text.
    #[arg(long)]
    json: bool,
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
    let cli = Cli::parse();

    let _guard = if let Some(path) = &cli.log_file {
        let file_appender = tracing_appender::rolling::never(
            path.parent().unwrap_or_else(|| std::path::Path::new(".")),
            path.file_name().unwrap_or_default(),
        );
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .with_writer(non_blocking)
            .init();
        Some(guard)
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .init();
        None
    };

    // Create gh client early for init (before config is loaded).
    let gh_init: std::sync::Arc<dyn forza::github::GitHubClient> =
        std::sync::Arc::new(forza::github::GhCliClient::new());

    if let Command::Init(args) = cli.command {
        return cmd_init(args, &*gh_init).await;
    }

    let config = match load_config(&cli.config) {
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
        Command::Explain(args) => cmd_explain(args, &config),
    }
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

    let rd = match forza::isolation::find_or_clone_repo(repo_str, explicit_dir, git).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return Err(ExitCode::FAILURE);
        }
    };

    Ok((repo_str.to_string(), rd, routes))
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

async fn cmd_run(
    args: RunArgs,
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
        repos_resolved.push((repo.to_string(), rd, routes.clone()));
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

fn cmd_explain(args: ExplainArgs, config: &forza::RunnerConfig) -> ExitCode {
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

    let repos = config.iter_repos();
    for (repo_slug, _repo_dir, routes) in &repos {
        if let Some(ref filter) = args.repo
            && repo_slug != filter
        {
            continue;
        }

        println!("Repository: {repo_slug}");
        println!("{}", "-".repeat(60));

        if routes.is_empty() {
            println!("  (no routes configured)");
        }

        for (route_name, route) in routes.iter() {
            println!();
            println!("  Route: {route_name}");

            // Trigger
            if let Some(ref label) = route.label {
                let gate = config
                    .global
                    .gate_label
                    .as_deref()
                    .map(|g| format!(" + gate label \"{g}\""))
                    .unwrap_or_default();
                println!("    Trigger:    label \"{label}\"{gate}");
            } else if let Some(ref cond) = route.condition {
                let cond_name = match cond {
                    forza::config::RouteCondition::CiFailing => "ci_failing",
                    forza::config::RouteCondition::HasConflicts => "has_conflicts",
                    forza::config::RouteCondition::CiFailingOrConflicts => {
                        "ci_failing_or_conflicts"
                    }
                    forza::config::RouteCondition::ApprovedAndGreen => "approved_and_green",
                    forza::config::RouteCondition::CiGreenNoObjections => "ci_green_no_objections",
                    forza::config::RouteCondition::AnyActionable => "any_actionable",
                };
                println!("    Trigger:    condition {cond_name}");
                println!("    Poll:       {}s", route.poll_interval);
            }

            // Type
            let route_type = match route.route_type {
                forza::config::SubjectType::Issue => "issue",
                forza::config::SubjectType::Pr => "pr",
            };
            println!("    Type:       {route_type}");

            // Workflow + stages
            let wf_name = route.workflow.as_deref().unwrap_or("(none)");
            if let Some(template) = config.resolve_workflow(wf_name) {
                let mode = match template.mode {
                    forza::workflow::WorkflowMode::Linear => "linear",
                    forza::workflow::WorkflowMode::Reactive => "reactive",
                };
                println!("    Workflow:   {wf_name} ({mode})");

                let stage_parts: Vec<String> = template
                    .stages
                    .iter()
                    .map(|s| {
                        let mut label = s.kind.name().to_string();
                        if s.agentless {
                            label.push_str(" (agentless)");
                        }
                        if s.condition.is_some() {
                            label.push_str(" (conditional)");
                        }
                        if s.optional {
                            format!("[{label}]")
                        } else {
                            label
                        }
                    })
                    .collect();
                println!("    Stages:     {}", stage_parts.join(" -> "));
            } else {
                println!("    Workflow:   {wf_name} (unresolved)");
            }

            // Retries
            let retries = route
                .max_retries
                .map(|r| r.to_string())
                .unwrap_or_else(|| "default (2 per stage)".into());
            println!("    Retries:    {retries}");

            // Model
            let model = config
                .effective_model(route)
                .unwrap_or("(none)")
                .to_string();
            println!("    Model:      {model}");

            // Skills
            let skills = config.effective_skills(route, None);
            if skills.is_empty() {
                println!("    Skills:     (none)");
            } else {
                println!("    Skills:     {}", skills.join(", "));
            }

            // Validation
            let validation = config.effective_validation(route);
            if validation.is_empty() {
                println!("    Validation: (none)");
            } else {
                for (i, cmd) in validation.iter().enumerate() {
                    if i == 0 {
                        println!("    Validation: {cmd}");
                    } else {
                        println!("                {cmd}");
                    }
                }
            }

            // On failure label
            println!(
                "    On failure: {} label applied",
                config.global.failed_label
            );
        }

        println!();
    }

    // Global section
    println!("Global");
    println!("{}", "-".repeat(60));
    println!("  Max concurrency: {}", config.global.max_concurrency);
    println!("  Auto-merge:      {}", config.global.auto_merge);
    println!("  Draft PR:        {}", config.global.draft_pr);
    println!("  Security level:  {}", config.security.authorization_level);

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

    println!("forza {}", env!("CARGO_PKG_VERSION"));
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
        println!("forza {}", env!("CARGO_PKG_VERSION"));
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
        env!("CARGO_PKG_VERSION"),
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
        println!("  {:<15} {:?}{duration}{cost}", stage.kind_name(), stage.status);
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
        env!("CARGO_PKG_VERSION"),
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
