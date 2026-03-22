# forza — agent context

forza is a GitHub automation runner that processes issues and PRs through configurable
multi-stage workflows executed by Claude. This file provides context for AI agents
(GitHub Copilot, Claude Code, and others) working on this codebase.

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

Key modules:
- `src/config.rs` — config structs, `SubjectType`, `RouteCondition`
- `src/workflow.rs` — `Stage`, `StageKind`, `WorkflowTemplate`, `WorkflowMode`
- `src/planner.rs` — build stage prompts, breadcrumb instructions
- `src/orchestrator/mod.rs` — execute stages, load breadcrumbs, fire hooks
- `src/orchestrator/helpers.rs` — PR body building, open_pr handling
- `src/state.rs` — `RunRecord`, `RouteOutcome`, run persistence

## Development

### Prerequisites

- Rust (MSRV: 1.90.0)
- `gh` CLI (authenticated)
- `git`

### Build and test

```bash
cargo build
cargo test
```

### Pre-commit checks (all must pass before opening a PR)

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --lib --all-features
cargo test --test '*' --all-features
cargo doc --no-deps --all-features
```

## Code conventions

- Rust 2024 edition — use if-let chains (`if let Some(x) = y && condition {`) instead of nested blocks
- `thiserror` for library errors, `anyhow` for application errors
- All public APIs must have doc comments
- No emojis in code, commits, or documentation

## Testing

All tests are inline unit tests in `#[cfg(test)]` modules within the source files. There
are no separate integration test files. Tests are self-contained and use `tempfile` for
filesystem fixtures where needed.

Key test coverage by module:

| Module | Focus |
|--------|-------|
| `src/config.rs` | Config parsing, validation, route resolution, effective skills |
| `src/workflow.rs` | Stage/workflow construction, StageKind parsing |
| `src/state.rs` | RunRecord serialization, RouteOutcome formatting |
| `src/planner.rs` | Prompt assembly, breadcrumb instruction injection |
| `src/orchestrator/mod.rs` | Stage execution logic, hook ordering |
| `src/notifications.rs` | Notification formatting |

## Commit style

Use [conventional commits](https://www.conventionalcommits.org/):

```
feat: add schedule window support
fix(orchestrator): handle stale lease on startup
docs: update CLI reference in README
```

Breaking changes use `feat!:` or `fix!:`.

## Branch naming

- `fix/` — bug fixes
- `feat/` — new features
- `docs/` — documentation
- `refactor/` — code refactoring
- `test/` — test improvements

## Config structure

```toml
[global]
model = "claude-sonnet-4-6"
gate_label = "forza:ready"
branch_pattern = "automation/{issue}-{slug}"

[security]
authorization_level = "trusted"       # sandbox | local | contributor | trusted

[validation]
commands = ["cargo fmt --all -- --check", "cargo clippy --all-targets -- -D warnings"]

[repos."owner/name"]
[repos."owner/name".routes.route-name]
type = "issue"          # or "pr"
label = "bug"           # label trigger (mutually exclusive with condition)
workflow = "bug"        # workflow template name
```

## Stage kinds

12 stage kinds: `triage`, `clarify`, `plan`, `implement`, `test`, `review`, `open_pr`,
`revise_pr`, `fix_ci`, `merge`, `research`, `comment`.

## RouteOutcome variants

| Variant | When set |
|---------|----------|
| `PrCreated` | Issue workflow completed and a new PR was opened |
| `PrUpdated` | Existing PR was updated (rebased, CI fixed, etc.) |
| `PrMerged` | PR was successfully merged |
| `CommentPosted` | Workflow posted a comment (e.g., research route) |
| `NothingToDo` | Reactive/condition route found no action was needed |
| `Failed` | Run failed at the named stage |
| `Exhausted` | Retry budget exhausted — `forza:needs-human` applied |
