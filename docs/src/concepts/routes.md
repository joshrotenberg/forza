# Routes

A route is a named rule that maps a trigger to a workflow. Routes are defined per repository under `[repos."owner/name".routes.<name>]`.

## Trigger types

### Label routes

Fire when a GitHub label is present on an issue or PR:

```toml
[repos."owner/name".routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"
```

### Condition routes

Fire automatically when PR state matches a condition — no label required:

```toml
[repos."owner/name".routes.auto-fix]
type = "pr"
condition = "ci_failing_or_conflicts"
workflow = "pr-fix"
scope = "forza_owned"
max_retries = 3
```

Supported conditions:

| Condition | When it fires |
|-----------|--------------|
| `ci_failing` | CI checks are failing on the PR |
| `has_conflicts` | The PR has merge conflicts |
| `ci_failing_or_conflicts` | Either CI failing or conflicts present |
| `approved_and_green` | PR is approved and CI is passing |
| `ci_green_no_objections` | CI is passing with no blocking reviews |

## Route fields

| Field | Type | Description |
|-------|------|-------------|
| `type` | `"issue"` \| `"pr"` | Whether this route matches issues or PRs |
| `label` | string | Label trigger (label routes only) |
| `condition` | string | Condition trigger (condition routes only) |
| `workflow` | string | Workflow template name to execute |
| `scope` | `"forza_owned"` \| `"all"` | Which PRs to evaluate (condition routes only; default: `forza_owned`) |
| `concurrency` | integer | Maximum parallel runs for this route |
| `poll_interval` | integer | Seconds between discovery polls |
| `max_retries` | integer | Max failures before applying `forza:needs-human` |
| `branch_pattern` | string | Override global `branch_pattern` for this route (see [Configuration Reference](../configuration/reference.md#branch-pattern-placeholders)) |
| `model` | string | Model override for this route |
| `skills` | string[] | Additional skill files to inject into agent prompts |

## Branch pattern

Each route inherits the global `branch_pattern` by default. Set `branch_pattern` on a route to override it:

```toml
[global]
branch_pattern = "automation/{issue}-{slug}"

[repos."owner/name".routes.bugfix]
# Inherits global pattern: automation/42-fix-bug

[repos."owner/name".routes.design-review]
branch_pattern = "review/{issue}-{slug}"
# Produces: review/42-fix-bug — different prefix
```

Per-route patterns are useful for separating branch namespaces. For example, a `design-review` route with prefix `review/` won't be picked up by condition routes that use `forza_owned` scope, because `forza_owned` checks branch prefixes (see below).

## Scope

Condition routes default to `forza_owned` — only PRs on branches that forza created are evaluated. Set `scope = "all"` to evaluate every open PR in the repository.

Label routes always evaluate every issue or PR with the matching label, regardless of origin.

### Multi-prefix `forza_owned` matching

The `forza_owned` scope determines ownership by checking whether a PR's branch starts with any known forza branch prefix. Forza collects prefixes from the global `branch_pattern` and every per-route `branch_pattern` override, deduplicates them, and matches against all of them.

For example, given this config:

```toml
[global]
branch_pattern = "automation/{issue}-{slug}"    # prefix: "automation/"

[repos."owner/name".routes.bugfix]
# Inherits global — no new prefix

[repos."owner/name".routes.hotfix]
branch_pattern = "fix/{issue}-{slug}"           # prefix: "fix/"

[repos."owner/name".routes.auto-rebase]
type = "pr"
condition = "has_conflicts"
scope = "forza_owned"
```

The `auto-rebase` route evaluates PRs on branches starting with `automation/` or `fix/` — covering branches created by both the `bugfix` and `hotfix` routes.

To exclude a route's branches from condition-triggered maintenance, give it a prefix that differs from all other routes:

```toml
[repos."owner/name".routes.design-review]
branch_pattern = "review/{issue}-{slug}"
# auto-rebase won't touch these — "review/" isn't a forza_owned prefix
# unless another route also uses "review/"
```

## Concurrency

Each route has its own concurrency limit. Multiple routes can run concurrently up to `[global].max_concurrency`. A route with `concurrency = 1` processes one issue at a time within that route, reducing merge conflicts.

## Max retries

When a route's run fails, forza increments a retry counter for that subject. Once `max_retries` is reached, forza applies the `forza:needs-human` label and stops processing that subject, preserving it for manual review.
