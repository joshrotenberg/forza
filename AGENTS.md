# forza — agent context

forza is a GitHub automation runner that processes issues and PRs through configurable
multi-stage workflows. Agent-agnostic — supports Claude and Codex backends. This file
provides context for AI agents (GitHub Copilot, Claude Code, Codex) working on the codebase.

## Workspace structure

Two crates:
- **`crates/forza-core/`** — library crate with domain model, traits, and pipeline
- **`crates/forza/`** — binary crate with CLI, API, MCP, and client implementations

Key modules:
- `crates/forza-core/src/pipeline.rs` — unified `execute()` function for all subjects
- `crates/forza-core/src/stage.rs` — `StageKind`, `Stage`, `Workflow`, builtin templates
- `crates/forza-core/src/subject.rs` — `Subject` (unified issue/PR type)
- `crates/forza-core/src/traits.rs` — `GitHubClient`, `GitClient`, `AgentExecutor` traits
- `crates/forza-core/src/testing.rs` — `MockGitHub`, `MockGit`, `MockAgent`
- `crates/forza/src/runner.rs` — discovery, scheduling, pipeline execution
- `crates/forza/src/adapters.rs` — `ClaudeAgentAdapter`, `CodexAgentAdapter`
- `crates/forza/src/config.rs` — `RunnerConfig`, `Route`, TOML parsing

## Development

### Prerequisites

- Rust (MSRV: 1.90.0, edition 2024)
- `gh` CLI (authenticated)
- `git`

### Build and test

```bash
cargo build --all
cargo test --all
```

### Pre-push checks (all must pass)

```bash
cargo fmt --all -- --check
cargo clippy --all --all-targets -- -D warnings
cargo test --all
cargo doc --no-deps --all-features
```

## Code conventions

- Rust 2024 edition — use if-let chains (`if let Some(x) = y && condition {`)
- `thiserror` for errors in both crates
- All public APIs must have doc comments
- No emojis in code, commits, or documentation
- Prefer editing existing files over creating new ones

## Testing

Unit tests in `#[cfg(test)]` modules, plus integration tests:

| Location | Focus |
|----------|-------|
| `crates/forza-core/src/*.rs` | Core type tests (Subject, Stage, Route, Run, Condition) |
| `crates/forza-core/tests/pipeline_integration.rs` | Pipeline with MockGitHub/MockGit/MockAgent |
| `crates/forza/src/config.rs` | Config parsing, route matching |
| `crates/forza/src/runner.rs` | Branch generation |
| `crates/forza/tests/orchestrator.rs` | Route matching, serialization |

## Commit style

[Conventional commits](https://www.conventionalcommits.org/):

```
feat(forza-core): add DraftPr stage kind
fix(runner): handle stale lease on startup
docs: update CLI reference in README
refactor(adapters): rename AgentAdapter to ClaudeAgentAdapter
```

Scopes: `forza-core`, `runner`, `pipeline`, `config`, `adapters`, `github`, `git`.
Breaking changes use `feat!:` or `fix!:`.

## Branch naming

- `fix/` — bug fixes
- `feat/` — new features
- `docs/` — documentation
- `refactor/` — code refactoring
- `test/` — test improvements

## Stage kinds

13 stage kinds: `triage`, `clarify`, `plan`, `implement`, `test`, `review`, `open_pr`,
`revise_pr`, `fix_ci`, `merge`, `research`, `comment`, `draft_pr`.

## Outcome variants

| Variant | When set |
|---------|----------|
| `PrCreated` | Issue workflow completed and a new PR was opened |
| `PrUpdated` | Existing PR was updated (rebased, CI fixed) |
| `PrMerged` | PR was successfully merged |
| `CommentPosted` | Workflow posted a comment (research route) |
| `NothingToDo` | No action was needed this cycle |
| `Failed` | Run failed at the named stage |
| `Exhausted` | Retry budget exhausted, `forza:needs-human` applied |
