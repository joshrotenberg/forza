# Concepts

Forza is built around a small set of composable primitives. Understanding how they fit together makes configuration straightforward and debugging predictable.

## The execution model

```
Issue/PR  ->  Route  ->  Workflow  ->  Stages  ->  Output
```

1. **Discovery** — forza fetches open issues and PRs from GitHub
2. **Route matching** — each subject is compared against configured routes; the first match wins
3. **Workflow execution** — the matched route's workflow runs its stages in order
4. **Output** — forza opens a PR, posts a comment, merges, or records a failure

## Core primitives

| Primitive | What it is |
|-----------|-----------|
| [Route](routes.md) | A named rule mapping a trigger to a workflow |
| [Workflow](workflows.md) | A chain of stages for a type of work |
| [Stage](stage-kinds.md) | A bounded unit of work (agent-driven or agentless) |
| [Lifecycle](lifecycle.md) | GitHub label state machine tracking progress |
| [Breadcrumbs](breadcrumbs.md) | Context summaries passed between stages |

## Three actors, clear lanes

Forza is infrastructure, not an autonomous agent. Three actors share responsibility, and keeping their lanes clear is the key design constraint.

### Forza decides (the process)

- Which stages run and in what order
- When to stop (validation failure, retry budget)
- Where work happens (worktrees, branches)
- What labels to apply and when
- How to recover (condition routes, retry with backoff)

### Humans decide (the direction)

- What to work on (labeling issues `forza:ready`)
- When to start (manual run, watch mode, or action trigger)
- What "done" looks like (acceptance criteria in the issue)
- Whether to merge (or let auto-merge handle it)
- Sequencing and prioritization of work

### The agent decides (the implementation)

- How to implement the plan
- What files to change
- How to fix failures
- What to write in the PR

Forza provides deterministic guardrails — stages, validation, lifecycle labels, retry budgets — and the human decides what to work on and when. The agent decides how. Adding intelligence to the framework adds unpredictability.

## Design principles

These follow directly from the three-actor model.

1. **One route, one action, one cycle.** A route does exactly one thing. If a PR needs rebasing and CI fixing and merging, that is three routes across three poll cycles. Multi-step behavior is achieved through configuration, not framework complexity.

2. **GitHub is the state machine.** PR state on GitHub is authoritative. Routes are transition functions. The poll loop is the event loop. Forza does not maintain an internal state machine that duplicates or diverges from GitHub state.

3. **Match once, carry through.** Once a subject is matched to a route, that binding is immutable for the duration of the run. No re-matching mid-execution. The human's intent, expressed as a label or PR state, is locked in at discovery time.

4. **Config is behavior.** Everything a route does is visible in `forza.toml`. No global flags that secretly modify stage behavior. If you want auto-merge, include the merge stage. If you do not want it, leave it out.

5. **All workflows are linear.** Issues and PRs follow the same execution pipeline. Differences are data, not control flow. Stages execute in order. Fail-fast on non-optional failures. No reactive dispatch or branching.
