{preamble}

Fix CI failures on PR #{pr_number} in {repo}.

PR title: {pr_title}

Branch: `{head_branch}`

## Steps

1. Check the current CI status: `gh pr checks {pr_number} --repo {repo}`
2. Read the failure logs to understand what is failing.
3. Fix the failing checks in the source files.
4. Run the relevant validation commands locally to confirm the fix.
5. Commit the fix and push: `git push --force-with-lease origin {head_branch}`

Focus only on fixing CI failures — do not add unrelated changes.
