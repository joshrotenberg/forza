# forza

[![Crates.io](https://img.shields.io/crates/v/forza.svg)](https://crates.io/crates/forza)
[![docs.rs](https://docs.rs/forza/badge.svg)](https://docs.rs/forza)
[![License](https://img.shields.io/crates/l/forza.svg)](LICENSE-MIT)
[![CI](https://github.com/joshrotenberg/forza/actions/workflows/ci.yml/badge.svg)](https://github.com/joshrotenberg/forza/actions/workflows/ci.yml)

Autonomous GitHub issue runner. Turns issues into pull requests through configurable multi-stage workflows.

## How it works

```
GitHub Issue  ->  Route Match  ->  Workflow  ->  Stages  ->  Pull Request  ->  Auto-Merge
```

Point it at a repo, it picks up labeled issues, matches them to routes, executes staged workflows (plan, implement, test, review), opens PRs, and optionally auto-merges them. Each stage runs a bounded agent session in an isolated git worktree.

For PRs, condition routes automatically detect CI failures and merge conflicts, fix them reactively, and merge when green.

## Quick start

```bash
# Install
cargo install forza

# Initialize a repo (creates labels and starter config)
forza init --repo owner/name

# Process a single issue
forza issue 123

# Preview without executing
forza issue 123 --dry-run

# Fix a PR (rebase + fix CI)
forza pr 42

# Poll for eligible issues (one batch)
forza run

# Watch mode (continuous polling + auto-fix)
forza watch --interval 60
```

## Configuration

Forza uses `forza.toml` with named routes per repo:

```toml
[global]
model = "claude-sonnet-4-6"
gate_label = "forza:ready"
branch_pattern = "automation/{issue}-{slug}"
auto_merge = true

[security]
authorization_level = "trusted"

[validation]
commands = [
    "cargo fmt --all -- --check",
    "cargo clippy --all-targets -- -D warnings",
    "cargo test --lib",
]

[repos."owner/name"]

# Issue routes — triggered by labels
[repos."owner/name".routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"
concurrency = 1

[repos."owner/name".routes.features]
type = "issue"
label = "enhancement"
workflow = "feature"
concurrency = 2

# PR routes — triggered by labels or conditions
[repos."owner/name".routes.fix-pr]
type = "pr"
label = "forza:fix-pr"
workflow = "pr-fix"

# Condition route — auto-detects and fixes broken PRs
[repos."owner/name".routes.auto-fix]
type = "pr"
condition = "ci_failing_or_conflicts"
workflow = "pr-maintenance"
scope = "forza_owned"
max_retries = 3
```

## Concepts

### Route

A named rule that maps a trigger to a workflow. Two types of triggers:

- **Label routes**: fire when a GitHub label is present on an issue or PR
- **Condition routes**: fire when PR state matches (CI failing, merge conflicts, approved and green)

Each route has its own concurrency limit, polling interval, and optional schedule window.

### Workflow Template

A chain of stages for a type of work. Built-in templates:

| Template | Mode | Stages |
|----------|------|--------|
| **bug** | Linear | plan -> implement -> test -> review -> open_pr -> merge |
| **feature** | Linear | plan -> implement -> test -> review -> open_pr -> merge |
| **chore** | Linear | implement -> test -> open_pr -> merge |
| **research** | Linear | research -> comment |
| **pr-fix** | Linear | revise_pr -> fix_ci -> merge |
| **pr-fix-ci** | Linear | fix_ci -> merge |
| **pr-rebase** | Linear | revise_pr -> merge |
| **pr-maintenance** | Reactive | conflicts? -> fix_ci? -> review? -> merge? |

### Stage

A bounded unit of work. 12 stage kinds: `triage`, `clarify`, `plan`, `implement`, `test`, `review`, `open_pr`, `revise_pr`, `fix_ci`, `merge`, `research`, `comment`.

Stages can be:
- **Agentless**: run a shell command directly (no agent invocation)
- **Conditional**: gated by a shell command exit code
- **Optional**: skippable without failing the run

### Workflow Modes

- **Linear**: stages execute in order, fail-fast on non-optional failures
- **Reactive**: each poll cycle evaluates stage conditions, runs the first match (one action per cycle)

### Labels

GitHub labels drive the lifecycle: `forza:ready` -> `forza:in-progress` -> `forza:complete` / `forza:failed` / `forza:needs-human`.

### Route Outcomes

Every run records what it produced: `PrCreated`, `PrUpdated`, `PrMerged`, `CommentPosted`, `Failed`, `Exhausted`, `NothingToDo`.

## Architecture

Three separated layers:

```
+-------------------------------------------------+
|  Platform (github.rs)                           |
|  Issues, PRs, comments, labels via gh CLI       |
+-------------------------------------------------+
|  Domain (config, planner, workflow)             |
|  Route matching, orchestration, scheduling      |
+-------------------------------------------------+
|  Execution (executor, isolation)                |
|  Agent invocation, worktree isolation           |
+-------------------------------------------------+
```

Agent-agnostic via the `AgentAdapter` trait. Claude is the default, pluggable for other agents.

## CLI

```
forza init          Create labels and starter config
forza issue <N>     Process a single issue
forza pr <N>        Process a single PR (fix CI, rebase, etc.)
forza run           Process one batch of eligible issues
forza watch         Continuous polling with auto-fix
forza status        Show run history and outcomes
forza fix           Re-run failed stages with error context
forza clean         Remove worktrees and run state
forza serve         Start the REST API server
forza mcp           Start the MCP server (stdio)
```

## License

MIT OR Apache-2.0
