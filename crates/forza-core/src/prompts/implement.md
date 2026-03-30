{preamble}

Implement the changes for issue #{issue_number}.

{issue_title}
{comments}

## Context

If a plan breadcrumb exists at `.plan_breadcrumb.md`, read it for the list of files to modify and the approach. If it does not exist, use the issue title and description above to determine what to implement.

## Instructions

1. If the breadcrumb exists, only modify the files listed there. Otherwise, determine the minimal set of files to change from the issue description.
2. Follow the existing code patterns and style.
3. Follow the project's existing language idioms and conventions.
{validation_step}{commit_num}. Stage all changed files with `git add` and commit. If the breadcrumb contains a commit message, use it exactly. Otherwise, write a conventional-commit message that references the issue (e.g., `feat(module): short description closes #{issue_number}`).

Do NOT create a PR in this stage.
