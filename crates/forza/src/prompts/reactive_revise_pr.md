{preamble}

Revise PR #{pr_number}: {pr_title}

## Steps

1. Check for merge conflicts: `git fetch origin && git rebase origin/{base_branch}`
2. If the rebase has conflicts, resolve them. Read the conflicting files, understand both sides, and produce the correct merged result.
3. Check for review feedback: `gh pr view {pr_number} --json reviews`
4. Address any CHANGES_REQUESTED comments.
5. Commit any changes and push: `git push --force-with-lease origin {head_branch}`

Branch: `{head_branch}` -> `{base_branch}`{breadcrumb}