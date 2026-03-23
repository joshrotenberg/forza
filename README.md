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

## Design

See [design/principles.md](design/principles.md) for the design principles and feature evaluation guidelines.

## License

MIT OR Apache-2.0
