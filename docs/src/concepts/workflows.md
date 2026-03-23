# Workflows

A workflow is a chain of stages that execute in order. Forza ships with built-in workflow templates for common tasks. You can also define custom templates in `forza.toml`.

## Built-in templates

| Template | Stages |
|----------|--------|
| `bug` | plan -> draft_pr* -> implement -> test -> review -> open_pr -> merge* |
| `feature` | plan -> draft_pr* -> implement -> test -> review -> open_pr -> merge* |
| `chore` | implement -> test -> open_pr -> merge* |
| `research` | research -> comment |
| `pr-fix` | revise_pr -> fix_ci |
| `pr-fix-ci` | fix_ci |
| `pr-rebase` | revise_pr |
| `pr-merge` | merge (no worktree) |
| `pr-review` | review |

`*` = optional stage (skipped if not applicable, does not fail the run)

## Execution model

All workflows are linear. Stages run in the order defined. A non-optional stage failure stops the workflow and records a failure outcome. Optional stages are skipped without failure if their condition is not met.

Between stages, forza runs the configured [validation commands](../configuration/reference.md#validation). All validation commands must pass before the next stage starts.

## Custom workflow templates

Define custom templates in `forza.toml` using `[[workflow_templates]]`:

```toml
[[workflow_templates]]
name = "safe-feature"
stages = [
  { kind = "plan" },
  { kind = "implement" },
  { kind = "test", optional = true },
  { kind = "review" },
  { kind = "open_pr" },
]
```

A custom template with the same name as a built-in overrides the built-in.

## Stage options

Each stage in a template accepts these options:

| Option | Type | Description |
|--------|------|-------------|
| `kind` | string | The [stage kind](stage-kinds.md) to execute |
| `optional` | bool | If true, skip rather than fail when not applicable |
| `condition` | string | Shell command; skip stage if exit code is non-zero |
| `max_retries` | integer | Stage-level retry limit |
| `command` | string | For agentless stages: the shell command to run |

## Choosing a workflow

| Situation | Recommended workflow |
|-----------|---------------------|
| Bug fix with full lifecycle | `bug` |
| New feature with full lifecycle | `feature` |
| Dependency update or housekeeping | `chore` |
| Research question, no code change | `research` |
| Fix CI failures on an existing PR | `pr-fix-ci` |
| Rebase a PR with conflicts | `pr-rebase` |
| Fix CI and rebase | `pr-fix` |
| Merge an approved PR | `pr-merge` |
| Review a PR and leave feedback | `pr-review` |
