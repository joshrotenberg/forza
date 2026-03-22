# forza — agent context

forza is a GitHub automation runner that processes issues and PRs through configurable
multi-stage workflows executed by Claude. This file documents the configuration model
and key features for agents processing forza's own issues.

## Architecture

```
RunnerConfig (forza.toml / runner.toml)
  └─ repos."owner/name"
       └─ routes.name  ──→  WorkflowTemplate  ──→  Stage[]
                                                     ├─ kind (StageKind)
                                                     ├─ agentless / command
                                                     ├─ condition
                                                     ├─ skills / model / mcp_config
                                                     └─ optional / max_retries
```

Key modules: `src/config.rs` (config structs), `src/workflow.rs` (`Stage`, `StageKind`,
`WorkflowTemplate`), `src/planner.rs` (build stage prompts, breadcrumb instructions),
`src/orchestrator.rs` (execute stages, load breadcrumbs, fire hooks).

## Config structure

```toml
[global]
model = "claude-sonnet-4-6"
gate_label = "forza:ready"
branch_pattern = "automation/{issue}-{slug}"

[security]
authorization_level = "contributor"   # sandbox | local | contributor | trusted

[validation]
commands = ["cargo fmt --all -- --check", "cargo clippy --all-targets -- -D warnings"]

[repos."owner/name"]
[repos."owner/name".routes.route-name]
type = "issue"          # or "pr"
label = "bug"           # label trigger (mutually exclusive with condition)
workflow = "bug"        # workflow template name

[agent_config]
skills = ["./skills/rust.md"]
mcp_config = ".mcp.json"

[stage_hooks.implement]
pre    = ["..."]
post   = ["..."]
finally = ["..."]
```

## Breadcrumbs

Each stage that has a successor is instructed to write a context summary to
`.forza/breadcrumbs/{run_id}/{stage_name}.md` (e.g.,
`.forza/breadcrumbs/run-20260321-170349-1ff7fe81/plan.md`). The orchestrator reads
this file after the stage completes and prepends it as `## Context from previous stage`
to the next stage's prompt.

The plan stage is special: it writes to `.plan_breadcrumb.md` in the repo root (not
`.forza/breadcrumbs/`). The implement stage reads it for the file list and exact commit
message. Similarly `.review_breadcrumb.md` is written by review and read by open_pr.

## Agentless stages

Set `agentless = true` and `command = "..."` on a stage to run a shell command directly
instead of invoking Claude. Useful for formatting, linting, or scaffolding steps.

```toml
[[workflow_templates]]
name = "lint-first"
stages = [
  { kind = "implement", agentless = true, command = "cargo fmt --all" },
  { kind = "review" },
  { kind = "open_pr" },
]
```

The orchestrator runs the command via `sh -c` and records the output. No agent
invocation happens for agentless stages.

## Conditional stages

Set `condition = "..."` (a shell command) on a stage to gate its execution. Exit 0
means **skip** the stage; non-zero (or absent) means **run** it. Use together with
`optional = true` so a skipped stage does not fail the run. No hooks fire for skipped
stages.

```toml
{ kind = "test", optional = true, condition = "git diff --quiet HEAD~1 -- src/tests/" }
```

The condition is evaluated by the orchestrator before the stage starts.

## Per-stage hooks

Define hooks keyed by the snake_case `StageKind` name (`plan`, `implement`, `test`,
`review`, `open_pr`, `revise_pr`, `fix_ci`, `merge`, `research`, `comment`).

```toml
[stage_hooks.implement]
pre     = ["cargo check"]          # runs before the stage
post    = ["cargo fmt --all"]      # runs on success
finally = ["echo done"]            # runs regardless of outcome
```

`pre` failure aborts the stage. `post` failure marks the stage failed. `finally` always
runs (clean-up, notifications, etc.).

## Route-based config

Multi-repo config uses `[repos."owner/name".routes.name]`. Each route needs a `type`
(`issue` or `pr`) and at least one trigger (`label` or `condition`).

```toml
[repos."owner/name".routes.auto-fix]
type       = "pr"
condition  = "ci_failing_or_conflicts"   # RouteCondition: ci_failing | has_conflicts |
                                         #   ci_failing_or_conflicts | approved_and_green
workflow   = "pr-fix"
scope      = "forza_owned"               # forza_owned (default) | all
max_retries = 3                          # applies forza:needs-human after N failures
concurrency = 2
poll_interval = 60                       # check every minute for CI/conflict issues
model      = "claude-opus-4-6"           # per-route model override
skills     = ["./skills/pr-fix.md"]      # per-route skills override
mcp_config = ".mcp.json"
validation_commands = ["cargo test"]
```

Condition routes fire automatically based on PR state; no label is needed. Label routes
fire when the GitHub label is applied.

## RouteOutcome variants

Each run records its final outcome in `RunRecord::outcome`. The `RouteOutcome` enum
(defined in `src/state.rs`) has the following variants:

| Variant | Fields | When set |
|---------|--------|----------|
| `PrCreated` | `number: u64` | Issue workflow completed and a new PR was opened |
| `PrUpdated` | `number: u64` | Existing PR was updated (rebased, CI fixed, etc.) |
| `PrMerged` | `number: u64` | PR was successfully merged |
| `CommentPosted` | — | Workflow posted a comment (e.g., research route) |
| `NothingToDo` | — | Reactive/condition route found no action was needed |
| `Failed` | `stage: String`, `error: String` | Run failed at the named stage |
| `Exhausted` | `retries: usize` | Retry budget exhausted — `forza:needs-human` applied |

`format_outcome` in `src/main.rs` renders these for the status display (e.g.,
`pr_created (#42)`, `failed (stage: implement)`, `exhausted (3 retries)`).

## Testing

All tests are inline unit tests in `#[cfg(test)]` modules within the source files. There
are no integration tests. Run the full suite with:

```bash
cargo test
```

Key test coverage by module:

| Module | Focus |
|--------|-------|
| `src/config.rs` | Config parsing, validation, route resolution, effective skills |
| `src/workflow.rs` | Stage/workflow construction, StageKind parsing |
| `src/state.rs` | RunRecord serialization, RouteOutcome formatting |
| `src/planner.rs` | Prompt assembly, breadcrumb instruction injection |
| `src/orchestrator/mod.rs` | Stage execution logic, hook ordering |
| `src/notifications.rs` | Notification formatting |

Tests that require live GitHub API access or a running Claude process are not present;
all tests are self-contained and use `tempfile` for filesystem fixtures where needed.

## Skill injection

Skills are markdown files injected into the agent's context. Three levels of override
(stage > route > global):

```toml
[agent_config]
skills = ["./skills/rust.md"]          # global baseline

[repos."owner/name".routes.bugfix]
skills = ["./skills/bugfix.md"]        # overrides global for this route

# In a workflow template stage:
{ kind = "implement", skills = ["./skills/impl.md"] }   # overrides route for this stage
```

`RunnerConfig::effective_skills(route, stage_skills)` resolves the final list: stage
skills win if present, otherwise route skills, otherwise global.

## Current Status

**Version:** 0.2.0

### Implemented features

- **Full stage pipeline**: `triage`, `clarify`, `plan`, `implement`, `test`, `review`, `open_pr`, `revise_pr`, `fix_ci`, `merge`, `research`, `comment`
- **Workflow modes**: `linear` (sequential) and `reactive` (condition-evaluated dispatch loop)
- **Label routes**: fire when a GitHub label is applied to an issue or PR
- **Condition routes**: fire automatically on PR state (`ci_failing`, `has_conflicts`, `ci_failing_or_conflicts`, `approved_and_green`)
- **Agentless stages**: run shell commands directly (formatting, linting, scaffolding)
- **Conditional stages**: gate stage execution via shell command exit code; pair with `optional = true` to skip cleanly
- **Per-stage hooks**: `pre`, `post`, `finally` hooks keyed by `StageKind` name
- **Skill injection**: three-level override (global → route → stage)
- **Breadcrumbs**: inter-stage context hand-off via `.forza/breadcrumbs/{run_id}/{stage}.md`
- **Multi-retry with escalation**: `max_retries` per route; applies `forza:needs-human` on exhaustion
- **Notifications**: desktop, Slack webhook, and generic webhook on run completion
- **CLI**: `init`, `issue`, `pr`, `run`, `watch`, `status`, `fix`, `clean`, `serve`, `mcp`
- **REST API** (`serve`) and **MCP server** (stdio, `mcp`) for tool integration
- **Dependency validation**: checks `git`, `gh`, and the agent binary on startup
