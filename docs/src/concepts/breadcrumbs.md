# Breadcrumbs

Breadcrumbs are context summaries written by one stage and read by the next. They allow an agent to begin a new stage with knowledge of what previous stages discovered or decided, without requiring the agent to re-read the entire issue or codebase.

## How they work

Each stage with a successor writes a context summary to:

```
.forza/breadcrumbs/{run_id}/{stage_name}.md
```

The pipeline reads this file and prepends it as a `## Context from previous stage` section to the next stage's prompt.

## Plan breadcrumb

The `plan` stage writes an additional breadcrumb to the repository root:

```
.plan_breadcrumb.md
```

The `implement` stage reads this file to obtain the list of files to modify and the commit message. This is also the file used by forza's own automation agent to pick up where the plan stage left off.

## What goes in a breadcrumb

A breadcrumb typically contains:

- A summary of what this stage found or decided
- The files that were examined or changed
- Any design decisions or constraints the next stage should respect
- The proposed commit message (in the plan breadcrumb)

## Visibility

Breadcrumbs are stored in `.forza/breadcrumbs/` inside the git worktree created for the run. They are not committed to the repository — they exist only for the duration of the run and are cleaned up by `forza clean`.

The plan breadcrumb (`.plan_breadcrumb.md`) is written to the worktree root and is visible to agents operating in that directory.
