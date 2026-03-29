# Layers of Usage

forza is designed to be useful immediately with zero setup, and progressively more powerful as you add configuration. Each layer builds on the previous one.

## Layer 1: Direct Commands

**What you need:** forza installed, a GitHub repo.

No configuration file, no labels, no setup. Tell forza exactly what to do:

```bash
forza issue 42 --workflow quick
forza pr 84 --workflow pr-fix
```

forza infers the repository from the git remote, creates a worktree, runs the workflow stages, and opens a PR. You review and merge.

**Built-in workflows:**

| Workflow | Stages | Use for |
|----------|--------|---------|
| `quick` | implement, test, open_pr | Bugs, chores, small tasks |
| `feature` | plan, implement, test, review, open_pr | Larger features |
| `research` | research, comment | Investigation, no code changes |
| `pr-fix` | revise_pr, fix_ci | Fix PR review feedback + CI |
| `pr-rebase` | revise_pr | Rebase a PR |
| `pr-merge` | merge | Merge a PR |

**When to graduate to Layer 2:** you find yourself running forza on multiple issues with the same workflow, or you want forza to pick up work automatically.

## Layer 2: Config-Driven Automation

**What you need:** a `forza.toml` with routes.

Routes tell forza *when* to act, not just *how*. A route maps a trigger (label or condition) to a workflow:

```toml
[global]
model = "claude-sonnet-4-6"
gate_label = "forza:ready"

[repos."owner/name".routes.bugfix]
type = "issue"
label = "bug"
workflow = "quick"

[repos."owner/name".routes.auto-rebase]
type = "pr"
condition = "has_conflicts"
workflow = "pr-rebase"
scope = "forza_owned"
```

Now forza can discover work on its own:

```bash
forza run                    # process one batch
forza run --watch            # poll continuously
```

Label an issue `bug` + `forza:ready`, and the next poll cycle picks it up. Condition routes watch PRs and react to state changes (CI failing, merge conflicts, approved and green).

**When to graduate to Layer 3:** you have a batch of issues to work through and want to control the order.

## Layer 3: Planning

**What you need:** Layer 1 or 2, plus issues to plan.

`forza plan` analyzes multiple issues at once, detects dependencies, and creates a plan issue with a visual dependency graph:

```bash
forza plan 42 45 48 51       # analyze specific issues
forza plan --label backlog   # analyze by label
```

The plan issue is a GitHub issue with:
- A mermaid dependency graph (rendered by GitHub)
- Actionable issues in recommended order
- Blocked issues with reasons
- Skipped issues (already processed)

Review the plan. Comment to adjust. Revise:

```bash
forza plan --revise 99       # update plan based on your comments
```

Execute when ready:

```bash
forza plan --exec 99         # process issues in dependency order
forza plan --exec 99 --dry-run   # preview the order first
```

The plan is the contract between you and forza. You decide what to work on and approve the approach. Forza executes.

**When to graduate to Layer 4:** you want forza to run on GitHub's infrastructure instead of your laptop.

## Layer 4: GitHub Action

**What you need:** the forza action in your workflow, an `ANTHROPIC_API_KEY` secret.

Drop a workflow file in any repo:

```yaml
# .github/workflows/forza.yml
name: forza
on:
  issues:
    types: [labeled]
  check_suite:
    types: [completed]

permissions:
  contents: write
  issues: write
  pull-requests: write

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

Label an issue, the action fires, forza runs on GitHub's runners. No laptop, no polling, no infrastructure.

The action's auto mode maps events to commands:

| Event | forza command |
|-------|--------------|
| `issues.labeled` | `forza issue <N>` |
| `pull_request.labeled` | `forza pr <N>` |
| `check_suite.completed` | `forza run` |
| `schedule` | `forza run` |

**When to graduate to Layer 5:** you want automated testing of forza itself.

## Layer 5: Self-Testing

**What you need:** a test repo with scenario tags, the test-scenarios workflow.

forza can test itself. The test framework uses immutable git tags with known-broken code. Each test run:

1. Creates a throwaway branch from a scenario tag
2. Creates an issue describing the bug
3. Runs forza to fix it
4. Verifies the fix (build, tests pass)
5. Cleans up (close issue, close PR, delete branch)

The tag never changes. The test is infinitely repeatable. Multiple test runs can happen concurrently.

## Summary

| Layer | Config needed | Trigger | Runs on |
|-------|--------------|---------|---------|
| 1. Direct | None | You type a command | Your machine |
| 2. Automation | `forza.toml` | Labels, conditions | Your machine |
| 3. Planning | Issues to plan | `forza plan` | Your machine |
| 4. Action | Workflow file | GitHub events | GitHub runners |
| 5. Testing | Scenario tags | Schedule, dispatch | GitHub runners |

Start at Layer 1. Add layers as your needs grow.
