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
| `model` | string | Model override for this route |
| `skills` | string[] | Additional skill files to inject into agent prompts |

## Scope

Condition routes default to `forza_owned` — only PRs on branches that forza created are evaluated. Set `scope = "all"` to evaluate every open PR in the repository.

Label routes always evaluate every issue or PR with the matching label, regardless of origin.

## Concurrency

Each route has its own concurrency limit. Multiple routes can run concurrently up to `[global].max_concurrency`. A route with `concurrency = 1` processes one issue at a time within that route, reducing merge conflicts.

## Max retries

When a route's run fails, forza increments a retry counter for that subject. Once `max_retries` is reached, forza applies the `forza:needs-human` label and stops processing that subject, preserving it for manual review.
