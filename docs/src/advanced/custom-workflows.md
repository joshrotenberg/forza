# Custom Workflows

Forza's built-in workflow templates cover common cases. For specialized workflows, define custom templates in `forza.toml`.

## Defining a custom template

Add one or more `[[workflow_templates]]` sections to `forza.toml`:

```toml
[[workflow_templates]]
name = "hotfix"
stages = [
    { kind = "implement", max_retries = 1 },
    { kind = "test", max_retries = 2 },
    { kind = "open_pr" },
]
```

Then reference it from a route:

```toml
[repos."owner/name".routes.hotfix]
type = "issue"
label = "hotfix"
workflow = "hotfix"
```

A custom template with the same name as a built-in overrides the built-in for all routes.

## Agentless stages

Provide a `command` field to run a shell command instead of invoking the agent:

```toml
[[workflow_templates]]
name = "format-pr"
stages = [
    { kind = "implement", command = "cargo fmt --all" },
    { kind = "open_pr" },
]
```

Use agentless stages for deterministic operations (formatting, linting, dependency updates) that do not require reasoning.

## Conditional stages

Gate a stage on a shell command exit code. A non-zero exit skips the stage without failing the run:

```toml
[[workflow_templates]]
name = "test-if-changed"
stages = [
    { kind = "implement" },
    { kind = "test", condition = "git diff --quiet HEAD~1 -- tests/" },
    { kind = "open_pr" },
]
```

## Optional stages

Mark stages `optional = true` to skip them without failure:

```toml
[[workflow_templates]]
name = "safe-feature"
stages = [
    { kind = "plan" },
    { kind = "draft_pr", optional = true },
    { kind = "implement" },
    { kind = "test", optional = true },
    { kind = "review" },
    { kind = "open_pr" },
    { kind = "merge", optional = true },
]
```

## Research-only workflow

A workflow that investigates a question and posts findings as a comment, without opening a PR:

```toml
[[workflow_templates]]
name = "investigate"
stages = [
    { kind = "research" },
    { kind = "comment" },
]
```

## Combining options

Options compose freely. A stage can be agentless, conditional, and have a retry limit:

```toml
{ kind = "test", command = "cargo test --lib", condition = "cargo check 2>/dev/null", max_retries = 2, optional = true }
```

## Stage environment variables

All stage commands (agentless `command`, `condition`, hooks, validation) receive `FORZA_*` environment variables. See [Stage Kinds](../concepts/stage-kinds.md#environment-variables) for the full list.
