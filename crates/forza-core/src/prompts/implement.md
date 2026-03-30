{preamble}

Implement the changes for issue #{issue_number}.

{issue_title}
{comments}

## Instructions

1. If context from a previous planning stage was provided above, follow its file list and approach. Otherwise, determine the minimal set of files to change from the issue description.
2. Follow the existing code patterns and style.
3. Follow the project's existing language idioms and conventions.
{validation_step}{commit_num}. Stage all changed files with `git add` and commit. If a commit message was provided in the context above, use it exactly. Otherwise, write a conventional-commit message that references the issue (e.g., `feat(module): short description closes #{issue_number}`).

Do NOT create a PR in this stage.
