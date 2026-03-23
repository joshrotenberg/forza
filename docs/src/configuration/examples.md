# Configuration Examples

Ready-to-use configuration examples. Copy the one closest to your needs and adjust the repo name.

## Minimal

The simplest possible setup — one repo, one bug route, all defaults. Use this as your starting point.

```toml
{{#include ../../../examples/minimal.toml}}
```

## Rust project

Standard setup for a Rust codebase: fmt/clippy/test validation, three issue routes (bugs, features, chores), auto-fix condition route.

```toml
{{#include ../../../examples/rust-project.toml}}
```

## Multiple repositories

Manage several repos from a single forza instance. Each repo gets its own route table, concurrency limits, and polling frequencies.

```toml
{{#include ../../../examples/multi-repo.toml}}
```

## PR maintenance

Focused on keeping open PRs healthy. Condition routes fire automatically based on PR state — no label needed.

```toml
{{#include ../../../examples/pr-maintenance.toml}}
```

## Security-strict

A locked-down configuration for high-stakes or public-facing repos. Sandbox authorization, no auto-merge, explicit author allowlist, and a required approval label.

```toml
{{#include ../../../examples/security-strict.toml}}
```

## Full runner

Complete example showing all available options: multi-repo, cost limits, custom workflow templates, agent config, and per-stage hooks.

```toml
{{#include ../../../examples/runner.toml}}
```

## Self-hosting

For a complete real-world example, see [`forza.toml`](https://github.com/joshrotenberg/forza/blob/main/forza.toml) in the forza repository itself. It is the config forza uses to process its own issues and PRs, with bug, feature, chore, and research routes plus condition-based PR maintenance.
