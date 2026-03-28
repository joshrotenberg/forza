# Lifecycle

Forza uses GitHub labels to track the state of each issue or PR it processes. Labels are the state machine.

## Label states

```
forza:ready
    |
    v
forza:in-progress
    |
    +-- (success) --> forza:complete
    |
    +-- (failure) --> forza:failed
    |
    +-- (retries exhausted) --> forza:needs-human
```

| Label | Meaning |
|-------|---------|
| `forza:ready` | The issue/PR is eligible for processing (gate label) |
| `forza:in-progress` | Forza has picked up the subject and is working on it |
| `forza:complete` | The run completed successfully |
| `forza:failed` | The run failed; retry is possible |
| `forza:needs-human` | Retries exhausted or blocked during planning; manual intervention required |
| `forza:plan` | A plan issue created by `forza plan` |

## Gate label

When `gate_label` is configured (typically `forza:ready`), forza only processes issues and PRs that carry that label. At the start of a run, the gate label is replaced with `forza:in-progress`. This prevents double-processing and makes the current state visible in the GitHub UI.

Condition routes bypass the gate label — they fire based on PR state, not labels.

## Initialization

Run `forza init` to create all required labels in a repository:

```bash
forza init --repo owner/name
```

This is a one-time setup step. The command is idempotent — safe to run on a repo that already has some of the labels.

## Run outcomes

Every run records an outcome:

| Outcome | Description |
|---------|-------------|
| `PrCreated` | A new PR was opened |
| `PrUpdated` | An existing PR was updated |
| `PrMerged` | A PR was merged |
| `CommentPosted` | A comment was posted on the issue |
| `Failed` | The run failed with an error |
| `Exhausted` | Max retries reached; `forza:needs-human` applied |
| `NothingToDo` | No eligible subjects found |

Use `forza status` to view the outcome history for recent runs.
