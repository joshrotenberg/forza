{preamble}

Review PR #{pr_number} in {repo}.

PR title: {pr_title}

PR description:
{pr_body}

Branch: `{head_branch}` -> `{base_branch}`

## What to check

- Correctness: does the implementation look correct?
- Tests: are there tests for new behavior?
- Code quality: any obvious bugs, crashes, or unsafe operations?
- Consistency: does the style match the surrounding code?

## Output format

Post a review comment on the PR summarizing your findings:

```
gh pr review {pr_number} --repo {repo} --comment --body "..."
```

Do NOT modify any source files in this stage.
