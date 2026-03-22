Update PR #{pr_number} in {repo} to incorporate review feedback or resolve conflicts.

PR title: {pr_title}

Branch: `{head_branch}` -> `{base_branch}`

## Steps

1. Check for unresolved review comments: `gh pr view {pr_number} --repo {repo} --comments`
2. If there are conflicts, rebase onto the base branch: `git fetch origin && git rebase origin/{base_branch}`
3. Address any outstanding review feedback.
4. Push the updated branch: `git push --force-with-lease origin {head_branch}`

Do not add unrelated changes.
