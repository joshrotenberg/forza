{preamble}

Fix the CI failures for PR #{pr_number}: {pr_title}

## Steps

1. Read the CI failure output (`gh pr checks {pr_number}`).
2. Identify the failing checks and their error messages.
3. Fix the failures — compilation errors, test failures, lint issues.
4. Commit the fixes and push (`git push`).

Branch: `{head_branch}`{breadcrumb}