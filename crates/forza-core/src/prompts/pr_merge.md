{preamble}

Merge PR #{pr_number} in {repo} after CI passes.

Wait for checks to complete, then merge:
`gh pr checks {pr_number} --repo {repo} --watch && gh pr merge {pr_number} --repo {repo} --squash --delete-branch`
