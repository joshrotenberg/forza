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

When an issue is labeled (or a PR enters a matching state), forza:

1. **Picks it up** — the watch loop finds issues with `forza:ready` or PRs matching a condition route
2. **Matches a route** — compares the label or PR state against your configured routes to select a workflow
3. **Executes stages** — runs each stage in order (e.g., plan → implement → test → review → open_pr), with validation commands between stages and breadcrumbs carrying context forward
4. **Opens a PR** — commits the work to an isolated git worktree and opens a PR against your branch
5. **Merges (optional)** — if `auto_merge = true` and CI passes, the merge stage completes the lifecycle

For PRs, condition routes automatically detect CI failures and merge conflicts, fix them reactively, and merge when green.

### forza vs. running Claude directly

| | forza | Claude directly |
|---|---|---|
| Stage sequencing | Deterministic, configured per workflow | Ad-hoc, single session |
| Context hand-off | Breadcrumbs carry forward between stages | Manual copy-paste |
| Validation | Commands run between every stage | None by default |
| Retry / escalation | `max_retries` + `forza:needs-human` label | Manual |
| Worktree isolation | Each run gets a clean git worktree | Working directory |
| PR lifecycle | Automated open, update, merge | Manual |

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

## When to use forza

**Solo developer** — label a backlog issue `forza:ready` and let forza open a PR while you work on something else. Good for bug fixes, dependency updates, documentation PRs, and chores that follow a clear pattern.

**Team with an issue backlog** — apply `forza:ready` to issues you're comfortable automating. Forza triages, plans, and implements while the team focuses on review. Pair with `gate_label` so nothing runs until you've explicitly opted it in.

**CI maintenance** — configure condition routes (`ci_failing_or_conflicts`, `approved_and_green`) so forza watches your forza-owned PRs and fixes failures, rebases stale branches, and merges when green — without anyone having to manually re-trigger CI or click merge.

**Research and exploration** — use a `research` route with the `research -> comment` workflow. Forza investigates a question (API compatibility, migration path, alternative approaches) and posts findings directly on the issue as a comment. No code changes, no PR.

## Progressive complexity

### Minimal

Get started with a single repo and one label route. No validation, default workflow, no auto-merge.

```toml
[global]
model = "claude-sonnet-4-6"

[repos."owner/name"]

[repos."owner/name".routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"
```

### Standard

Add validation, multiple routes, and auto-merge. This is the recommended starting point for most projects.

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

[repos."owner/name".routes.fix-pr]
type = "pr"
label = "forza:fix-pr"
workflow = "pr-fix"

[repos."owner/name".routes.auto-fix]
type = "pr"
condition = "ci_failing_or_conflicts"
workflow = "pr-maintenance"
scope = "forza_owned"
max_retries = 3
```

### Advanced

Multiple repos, custom workflow templates, per-stage hooks, skill injection, and reactive PR maintenance.

```toml
[global]
model = "claude-sonnet-4-6"
gate_label = "forza:ready"
auto_merge = true
max_concurrency = 5

[security]
authorization_level = "trusted"

[validation]
commands = ["cargo test --all-features"]

[agent_config]
skills = ["./skills/rust.md"]
mcp_config = ".mcp.json"

[stage_hooks.implement]
post    = ["cargo fmt --all"]
finally = ["echo 'implement done'"]

# First repo — standard issue routes
[repos."org/backend"]

[repos."org/backend".routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"
concurrency = 1
skills = ["./skills/backend.md"]

[repos."org/backend".routes.auto-merge]
type = "pr"
condition = "ci_green_no_objections"
workflow = "pr-maintenance"
scope = "forza_owned"
max_retries = 2

# Second repo — docs and research only
[repos."org/docs"]

[repos."org/docs".routes.docs]
type = "issue"
label = "documentation"
workflow = "chore"
concurrency = 3

[repos."org/docs".routes.research]
type = "issue"
label = "research"
workflow = "research"
concurrency = 5

# Custom workflow template
[[workflow_templates]]
name = "safe-feature"
stages = [
  { kind = "plan" },
  { kind = "implement" },
  { kind = "test", optional = true, condition = "git diff --quiet HEAD~1 -- tests/" },
  { kind = "review" },
  { kind = "open_pr" },
]
```

### Self-hosting

For a complete real-world example, see [`forza.toml`](forza.toml) in this repository. It is the config forza uses to process its own issues and PRs, with bug, feature, chore, and research routes plus condition-based PR maintenance.

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
