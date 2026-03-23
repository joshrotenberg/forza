# forza

[![Crates.io](https://img.shields.io/crates/v/forza.svg)](https://crates.io/crates/forza)
[![License](https://img.shields.io/crates/l/forza.svg)](https://github.com/joshrotenberg/forza/blob/main/LICENSE-MIT)
[![CI](https://github.com/joshrotenberg/forza/actions/workflows/ci.yml/badge.svg)](https://github.com/joshrotenberg/forza/actions/workflows/ci.yml)

Autonomous GitHub issue runner. Turns issues into pull requests through configurable multi-stage workflows. Agent-agnostic — supports Claude and Codex backends.

## How it works

```
GitHub Issue  ->  Route Match  ->  Workflow  ->  Stages  ->  Pull Request  ->  Auto-Merge
```

When an issue is labeled (or a PR enters a matching state), forza:

1. **Picks it up** — the watch loop finds issues with `forza:ready` or PRs matching a condition route
2. **Matches a route** — compares the label or PR state against your configured routes to select a workflow
3. **Executes stages** — runs each stage in order (e.g., plan → implement → test → review → open_pr), with validation commands between stages and breadcrumbs carrying context forward
4. **Opens a PR** — commits the work to an isolated git worktree and opens a PR against your branch
5. **Merges (optional)** — if the workflow includes a merge stage, it waits for CI and merges

For PRs, condition routes automatically detect CI failures and merge conflicts, fix them, and merge when green. Each condition route does one thing — the poll loop handles sequencing across cycles.

## Responsibility boundaries

Forza is infrastructure, not an autonomous agent. Three actors share responsibility:

| Actor | Decides |
|-------|---------|
| **You (human)** | What to work on, when to start, what "done" means |
| **Forza (process)** | Which stages run, where work happens, when to stop, label lifecycle |
| **Agent (Claude/Codex)** | How to implement, what files to change, how to fix failures |

Adding decision-making to the framework — adaptive prompting, automatic workflow selection, intelligent retries — blurs these lanes and adds unpredictability. When evaluating a new feature, ask: does this belong with the human, the agent, or the framework? If it belongs with the human or agent, it does not belong in forza.

## When to use forza

**Solo developer** — label a backlog issue `forza:ready` and let forza open a PR while you work on something else. Good for bug fixes, dependency updates, documentation PRs, and chores that follow a clear pattern.

**Team with an issue backlog** — apply `forza:ready` to issues you're comfortable automating. Forza triages, plans, and implements while the team focuses on review. Pair with `gate_label` so nothing runs until you've explicitly opted it in.

**CI maintenance** — configure condition routes (`ci_failing_or_conflicts`, `approved_and_green`) so forza watches your forza-owned PRs and fixes failures, rebases stale branches, and merges when green — without anyone having to manually re-trigger CI or click merge.

**Research and exploration** — use a `research` route with the `research -> comment` workflow. Forza investigates a question (API compatibility, migration path, alternative approaches) and posts findings directly on the issue as a comment. No code changes, no PR.
