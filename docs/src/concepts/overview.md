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

## Design principles

**One route, one action.** Each route does exactly one thing. Multi-step PR maintenance uses separate routes across poll cycles, not branching logic inside a single route.

**Match once, carry through.** A subject is bound to its route at discovery time. No re-matching during execution.

**GitHub is the state machine.** PR state on GitHub is authoritative. Routes are transition functions; the poll loop is the event loop.

**All workflows are linear.** Stages execute in order. Fail-fast on non-optional failures. No reactive dispatch or branching.

See [design/principles.md](https://github.com/joshrotenberg/forza/blob/main/design/principles.md) in the repository for the full design rationale and feature evaluation guidelines.
