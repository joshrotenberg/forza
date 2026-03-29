# Workflow Patterns

Practical patterns for using forza day-to-day.

## Quick one-off

Got an idea? Let forza handle it.

```bash
forza issue 42 --workflow quick
```

No config needed. Forza reads the issue, implements a fix, runs tests, opens a PR. Review and merge. Done.

For bigger work that needs planning:

```bash
forza issue 42 --workflow feature
```

This adds a plan stage (analyzes the codebase before implementing) and a review stage (the agent reviews its own work before opening the PR).

## Background worker

Run forza continuously while you work on other things:

```bash
forza run --watch
```

The flow:

1. Create issues with clear acceptance criteria
2. Label them `forza:ready`
3. Forza picks them up, runs the workflow, opens PRs
4. Review PRs as they come in

For one-off batches instead of continuous polling:

```bash
forza run
```

## Batch planning

Have a backlog of issues? Plan them first:

```bash
# Create issues
forza open --prompt "add retry backoff to API calls"
forza open --prompt "fix null pointer in runner"
forza open --prompt "update README with new commands"

# Plan them
forza plan 42 43 44

# Review the plan issue — it has a mermaid dependency graph
# Comment to adjust ordering, move things around

# Execute in dependency order
forza plan --exec 45
```

The plan issue is the contract. You review it, comment on it, revise it. Forza executes what you approved.

## Work branch pattern

Keep agent work off main until you've reviewed it:

```bash
forza plan --exec 99 --branch work/sprint-42
```

All PRs from the plan target `work/sprint-42` instead of main. At the end of the day, review the branch diff and merge to main in one shot. All the speed of automation without unreviewed code on main.

## PR maintenance

Set up condition routes to keep PRs healthy automatically:

```toml
[repos."owner/name".routes.auto-rebase]
type = "pr"
condition = "has_conflicts"
workflow = "pr-rebase"
scope = "forza_owned"

[repos."owner/name".routes.auto-fix-ci]
type = "pr"
condition = "ci_failing"
workflow = "pr-fix-ci"
scope = "forza_owned"
```

With `forza run --watch`, these routes continuously monitor your PRs and fix conflicts, rebase stale branches, and fix CI failures — without anyone clicking buttons.

For one-off fixes without routes:

```bash
forza pr 84 --workflow pr-fix
```

## Research and exploration

Investigate a question without making code changes:

```bash
forza issue 50 --workflow research
```

The agent reads the codebase, investigates the question posed in the issue, and posts findings as a comment. No branch, no PR, no code changes.

Good for: API compatibility analysis, migration paths, architecture decisions, dependency evaluation.

## Multi-repo management

One config, multiple repos:

```toml
[repos."org/service-a".routes.bugfix]
type = "issue"
label = "bug"
workflow = "quick"

[repos."org/service-b".routes.features]
type = "issue"
label = "enhancement"
workflow = "feature"
```

One `forza run --watch` handles all configured repos. Use `forza explain` to verify routing:

```bash
forza explain --issues     # show all issue routes
forza explain --prs        # show all PR routes
```

## Handling failures

When a run fails, forza posts a comment on the issue explaining what went wrong. The usual fixes:

1. **Unclear issue** — edit the description, add acceptance criteria, re-run
2. **Transient failure** — re-run with error context: `forza issue 42 --fix`
3. **Persistent failure** — check `forza status` for patterns, narrow the issue scope

```bash
forza status                 # recent runs
forza issue 42 --fix         # retry with error context
```

A run that fails the same stage repeatedly usually means the issue needs more specificity.

## GitHub Action

Automate everything with a workflow file:

```yaml
name: forza
on:
  issues:
    types: [labeled]
  check_suite:
    types: [completed]

jobs:
  forza:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: joshrotenberg/forza/action@main
        env:
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

Label an issue, the action fires on GitHub's infrastructure. No laptop, no polling.
