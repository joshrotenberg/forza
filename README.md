# forza

[![Crates.io](https://img.shields.io/crates/v/forza.svg)](https://crates.io/crates/forza)
[![docs.rs](https://docs.rs/forza/badge.svg)](https://docs.rs/forza)
[![License](https://img.shields.io/crates/l/forza.svg)](LICENSE-MIT)
[![CI](https://github.com/joshrotenberg/forza/actions/workflows/ci.yml/badge.svg)](https://github.com/joshrotenberg/forza/actions/workflows/ci.yml)

Autonomous GitHub issue runner. Turns issues into pull requests through configurable multi-stage workflows. Agent-agnostic — supports Claude and Codex backends.

```
GitHub Issue  ->  Route Match  ->  Workflow  ->  Stages  ->  Pull Request
```

Label an issue `forza:ready`, configure a route, and forza plans, implements, tests, reviews, and opens a PR — then merges when CI is green.

## Quick start

```bash
# Install
cargo install forza

# Initialize a repo (creates labels and starter config)
forza init --repo owner/name

# Process a single issue
forza issue 123

# Continuous polling loop
forza watch --interval 60
```

## Planning

`forza plan` analyzes a set of issues and creates a plan issue with a dependency graph, implementation order, and readiness assessment.

```bash
# Plan all open issues
forza plan

# Plan specific issues
forza plan 42 45 48

# Revise a plan based on human feedback
forza plan --revise 99

# Execute a plan in dependency order
forza plan --exec 99
```

The plan issue includes a mermaid dependency graph (rendered by GitHub), actionable issues in order, blocked issues with reasons, and skipped issues. Blocked issues get `forza:needs-human` labels and explanatory comments.

Minimal `forza.toml`:

```toml
[global]
model = "claude-sonnet-4-6"

[repos."owner/name"]

[repos."owner/name".routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"
```

## Documentation

Full documentation at **[joshrotenberg.github.io/forza](https://joshrotenberg.github.io/forza)**, including:

- [Getting Started](https://joshrotenberg.github.io/forza/getting-started.html)
- [Concepts](https://joshrotenberg.github.io/forza/concepts/overview.html) — routes, workflows, stages, lifecycle
- [Configuration Reference](https://joshrotenberg.github.io/forza/configuration/reference.html)
- [Examples](https://joshrotenberg.github.io/forza/configuration/examples.html)
- [Writing Issues](https://joshrotenberg.github.io/forza/usage/writing-issues.html)
- [Security](https://joshrotenberg.github.io/forza/usage/security.html)

## What forza isn't

forza will fail, and that's by design. When a run fails, forza stops, labels the issue, and tells you what happened. It doesn't retry, work around, or guess.

- Not fully autonomous — humans decide what to work on and when
- Not self-healing — failures are reported, not automatically resolved
- Not a replacement for good issue writing — vague issues produce vague results
- Not an agent — forza is infrastructure that agents run inside
- Not trying to handle every edge case — simplicity and determinism over cleverness

See [design/principles.md](design/principles.md) for the full rationale.

## Design

See [design/principles.md](design/principles.md) for the design principles and feature evaluation guidelines.

## License

MIT OR Apache-2.0
