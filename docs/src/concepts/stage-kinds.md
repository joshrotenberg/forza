# Stage Kinds

There are 13 stage kinds. Each performs a specific, bounded unit of work.

## Issue workflow stages

| Kind | What the agent does |
|------|-------------------|
| `triage` | Classify the issue, add labels, determine if it is actionable |
| `clarify` | Post a comment asking for more information if the issue is ambiguous |
| `plan` | Produce a detailed implementation plan; writes a plan breadcrumb |
| `draft_pr` | Open a draft PR before implementation begins |
| `implement` | Make the code changes described in the plan |
| `test` | Write or update tests to cover the changes |
| `review` | Self-review the diff; flag concerns or verify correctness |
| `open_pr` | Commit to the worktree branch and open a PR |
| `research` | Investigate a question and produce findings |
| `comment` | Post a comment on the issue (typically after `research`) |

## PR workflow stages

| Kind | What the agent does |
|------|-------------------|
| `revise_pr` | Rebase the PR branch against the target branch |
| `fix_ci` | Diagnose CI failures and apply targeted fixes |
| `merge` | Merge the PR once CI is green and approvals are satisfied |

## Agentless stages

Any stage kind can be made agentless by providing a `command` field instead of invoking an agent:

```toml
[[workflow_templates]]
name = "format-only"
stages = [
  { kind = "implement", command = "cargo fmt --all" },
  { kind = "open_pr" },
]
```

An agentless stage runs the shell command directly and does not consume agent tokens.

## Conditional stages

A stage can be gated by a shell command. If the command exits non-zero, the stage is skipped:

```toml
{ kind = "test", condition = "git diff --quiet HEAD~1 -- tests/" }
```

This skips the `test` stage if no test files were modified.

## Optional stages

Mark a stage `optional = true` to skip it without failing the run when it is not applicable:

```toml
{ kind = "draft_pr", optional = true }
```

The `draft_pr` and `merge` stages in the built-in `bug` and `feature` templates are optional by default.

## Environment variables

All stages (agentless commands, conditions, hooks, validation) receive these environment variables:

| Variable | Value |
|----------|-------|
| `FORZA_REPO` | `owner/name` |
| `FORZA_SUBJECT_TYPE` | `issue` or `pr` |
| `FORZA_SUBJECT_NUMBER` | Issue or PR number |
| `FORZA_ISSUE_NUMBER` | Issue number (issue runs only) |
| `FORZA_PR_NUMBER` | PR number (PR runs only) |
| `FORZA_BRANCH` | The worktree branch name |
| `FORZA_RUN_ID` | Unique run identifier |
| `FORZA_ROUTE` | Route name |
| `FORZA_WORKFLOW` | Workflow template name |
