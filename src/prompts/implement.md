{preamble}

Implement the changes for issue #{issue_number}.

{issue_title}

## Context

Read the plan breadcrumb at `.plan_breadcrumb.md` for the list of files to modify and the approach decided in the plan stage.

## Instructions

1. Only modify the files listed in the breadcrumb. Do NOT touch any other files.
2. Follow the existing code patterns and style.
3. For Rust code: use Rust 2024 if-let chains — write `if let Some(x) = y && condition {` instead of nested if-let/if blocks.
{validation_step}{commit_num}. Commit using the exact commit message from the breadcrumb.

Do NOT create a PR in this stage.
