# forza

[![Crates.io](https://img.shields.io/crates/v/forza.svg)](https://crates.io/crates/forza)
[![docs.rs](https://docs.rs/forza/badge.svg)](https://docs.rs/forza)
[![License](https://img.shields.io/crates/l/forza.svg)](LICENSE-MIT)
[![CI](https://github.com/joshrotenberg/forza/actions/workflows/ci.yml/badge.svg)](https://github.com/joshrotenberg/forza/actions/workflows/ci.yml)

Configurable workflow orchestrator for agent driven software development.

## How it works

```
GitHub Issue  →  Triage  →  Workflow  →  Stages  →  Pull Request
```

Point it at a GitHub issue, it decides if the issue is ready, picks a workflow template, executes stages (plan, implement, test, review), and opens a PR. Each stage runs a bounded agent session in an isolated git worktree.

## Quick start

```bash
# Process a single issue
forza issue 123

# Preview without executing
forza issue 123 --dry-run
# Example output:
#   Would run workflow 'bug' for issue #123: Fix login crash
#   Stages: plan → implement → test → review → open_pr
#   Estimated cost: $0.80 - $1.50 (avg $1.05, based on 3 previous bug runs)
#   (cost line shown only when historical run data exists)

# Poll for eligible issues
forza run

# Watch mode (continuous polling)
forza watch --interval 300
```

## Configuration

Forza uses a `forza.toml` (or `runner.toml`) config with named routes:

```toml
[global]
repo = "owner/name"
model = "claude-sonnet-4-6"
gate_label = "runner:ready"

[routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"
concurrency = 1
poll_interval = 60

[routes.features]
type = "issue"
label = "enhancement"
workflow = "feature"
concurrency = 2
poll_interval = 300
```

## Concepts

### Route
A named lane that maps a type (issue/pr) + label to a workflow. Each route has its own concurrency and polling frequency.

### Workflow Template
The stage chain for a type of work. Built-in templates:

| Template | Stages |
|----------|--------|
| **bug** | plan → implement → test → review → open_pr |
| **feature** | plan → implement → test → review → open_pr |
| **chore** | implement → test → open_pr |
| **research** | research → comment |

### Stage
A bounded unit of work. Each stage has a kind, a prompt, tool scoping, and retry policy.

### Lease
GitHub labels that prevent duplicate work: `runner:ready` → `runner:in-progress` → `runner:complete`.

## Architecture

Three separated layers:

```
┌─────────────────────────────────────────────────────┐
│  Platform (github.rs)                                │
│  Issues, PRs, comments, labels via gh CLI            │
├─────────────────────────────────────────────────────┤
│  Domain (config, triage, planner, workflow)           │
│  Orchestration — what to do and when                 │
├─────────────────────────────────────────────────────┤
│  Execution (executor, isolation)                     │
│  Agent invocation and work isolation                 │
└─────────────────────────────────────────────────────┘
```

Agent-agnostic via the `AgentAdapter` trait. Claude is the default, pluggable for other agents.

## Status

MVP working. 12+ runs, 100% success rate. Processing its own issues to build itself.
# Test marker B — this will conflict
