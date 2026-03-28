# forza — agent context

forza is a GitHub automation runner that processes issues and PRs through configurable
multi-stage workflows. Agent-agnostic — supports Claude and Codex, configurable via
`agent = "claude"` or `agent = "codex"` in `forza.toml`.

## Workspace structure

```
Cargo.toml                        (workspace root)
crates/
  forza-core/                     (library — domain model, traits, pipeline)
    src/
      condition.rs                RouteCondition evaluation
      error.rs                    Error types
      lifecycle.rs                Label management (in-progress, complete, failed)
      pipeline.rs                 Unified stage execution (the core execute() function)
      planner.rs                  Prompt generation from Subject + Workflow
      route.rs                    Route, Trigger, Scope, MatchedWork
      run.rs                      Run, RunStatus, Outcome, StageRecord
      shell.rs                    Shell execution with FORZA_* env vars
      stage.rs                    StageKind, Stage, Execution, Workflow, builtins
      subject.rs                  Subject (unified issue/PR type), SubjectKind
      testing.rs                  MockGitHub, MockGit, MockAgent for tests
      traits.rs                   GitHubClient, GitClient, AgentExecutor traits
      commands/draft_pr.sh        Shell script for DraftPr stage
      prompts/*.md                Prompt templates (include_str! at compile time)
  forza/                          (binary — CLI, API, MCP, client implementations)
    src/
      adapters.rs                 ClaudeAgentAdapter, CodexAgentAdapter, GitHubAdapter, GitAdapter
      runner.rs                   Discovery, scheduling, pipeline execution
      config.rs                   RunnerConfig, Route, GlobalConfig (TOML parsing)
      executor.rs                 ClaudeAdapter (claude-wrapper integration)
      main.rs                     CLI (clap)
      api.rs                      REST API (axum)
      mcp.rs                      MCP server (tower-mcp)
      github/                     GitHubClient implementations (gh CLI, octocrab)
      git/                        GitClient implementations (git CLI, gix)
```

## Key design decisions

- **One route, one action.** Each route does exactly one thing. Multi-step PR
  maintenance uses separate routes across poll cycles.
- **Match once, carry through.** A subject is bound to its route at discovery
  time via `MatchedWork`. No re-matching during execution.
- **GitHub is the state machine.** PR state on GitHub is authoritative. Routes
  are transition functions, the poll loop is the event loop.
- **All workflows are linear.** Stages execute in order. No reactive dispatch.

## Config structure

```toml
[global]
agent = "claude"               # or "codex"
model = "claude-sonnet-4-6"
gate_label = "forza:ready"
branch_pattern = "automation/{issue}-{slug}"

[security]
authorization_level = "trusted"

[validation]
commands = ["cargo fmt --all -- --check", "cargo clippy --all-targets -- -D warnings"]

[repos."owner/name".routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"

[repos."owner/name".routes.auto-rebase]
type = "pr"
condition = "has_conflicts"       # ci_failing | has_conflicts | ci_failing_or_conflicts |
                                  # approved_and_green | ci_green_no_objections
workflow = "pr-rebase"
scope = "forza_owned"
max_retries = 3
```

## Builtin workflows

| Template | Stages |
|----------|--------|
| **bug** | plan -> draft_pr* -> implement -> test -> review -> open_pr -> merge* |
| **feature** | plan -> draft_pr* -> implement -> test -> review -> open_pr -> merge* |
| **chore** | implement -> test -> open_pr -> merge* |
| **research** | research -> comment |
| **pr-fix** | revise_pr -> fix_ci |
| **pr-fix-ci** | fix_ci |
| **pr-rebase** | revise_pr |
| **pr-merge** | merge (no worktree) |
| **pr-review** | review |

`*` = optional stage. Stage kinds: `triage`, `clarify`, `plan`, `implement`, `test`,
`review`, `open_pr`, `revise_pr`, `fix_ci`, `merge`, `research`, `comment`, `draft_pr`.

## Breadcrumbs

Each stage with a successor writes a context summary to
`.forza/breadcrumbs/{run_id}/{stage_name}.md`. The pipeline reads this and prepends
it as `## Context from previous stage` to the next stage's prompt.

The plan stage writes `.plan_breadcrumb.md` in the repo root. The implement stage
reads it for the file list and commit message.

## Execution pipeline

All work flows through `forza_core::pipeline::execute()` — one function for both
issues and PRs. The runner module handles discovery (fetching subjects from GitHub),
matching (binding to routes), and scheduling (concurrency limits via JoinSet).

Shell commands (agentless stages, conditions, hooks, validation) get `FORZA_*`
environment variables: `FORZA_REPO`, `FORZA_SUBJECT_TYPE`, `FORZA_SUBJECT_NUMBER`,
`FORZA_ISSUE_NUMBER`/`FORZA_PR_NUMBER`, `FORZA_BRANCH`, `FORZA_RUN_ID`,
`FORZA_ROUTE`, `FORZA_WORKFLOW`.

## Shell command trust boundary

All shell commands from `forza.toml` (validation, hooks, agentless commands,
conditions) run via `sh -c` with no sandboxing. The config file is the trust boundary.

## Testing

```bash
cargo test --all                    # all unit + integration tests
cargo test -p forza-core            # core library tests (106+)
cargo test -p forza --lib           # binary crate unit tests
cargo test -p forza-core --test pipeline_integration  # mock-based pipeline tests
```

Key test areas:
| Crate | Focus |
|-------|-------|
| `forza-core` | Subject, Stage, Workflow, Condition, Route, Run, Pipeline, Shell |
| `forza-core/tests/pipeline_integration.rs` | End-to-end pipeline with MockGitHub/MockGit/MockAgent |
| `forza` | Config parsing, route matching, state persistence |

## Pre-push checklist

```bash
cargo fmt --all -- --check
cargo clippy --all --all-targets -- -D warnings
cargo test --all
cargo doc --no-deps --all-features
```

## CLI reference

```
forza init          Create labels and generate starter config
forza issue <N>     Process a single issue
forza pr <N>        Process a single PR
forza run           Single batch cycle (discover + process)
forza watch         Continuous polling loop
forza status        View run history
forza explain       Visualize config, routes, and workflows
forza fix           Re-run failed stages
forza clean         Clean worktrees and state
forza serve         REST API server
forza mcp           MCP server (stdio)
forza plan          Create, revise, or execute a plan
```

`forza explain` supports filters: `--issues`, `--prs`, `--conditions`, `--route <name>`,
`--workflows`, `--workflow <name>`, `-v` (verbose), `--json`.

`forza plan` modes:
- `forza plan [issues]` — analyze issues, create a plan issue with mermaid dependency graph
- `forza plan --revise <N>` — revise plan issue #N based on human comments
- `forza plan --exec <N>` — execute plan issue #N in dependency order
- `--label`, `--limit`, `--model` flags for filtering and configuration
