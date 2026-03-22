{preamble}

Create or update a pull request for issue #{issue_number}.

## Steps

1. Push the branch to origin: `git push origin HEAD`
2. Read `.plan_breadcrumb.md` for the commit message and files changed.
3. Read `.review_breadcrumb.md` if it exists for the review verdict.
4. Check if a draft PR already exists on this branch: `gh pr list --head $(git branch --show-current) --json number,isDraft --jq '.[0]'`
5. If a draft PR exists, update it and mark it ready for review:
   ```
   gh pr edit <number> --title "<commit message>" --body "<PR body>"
   gh pr ready <number>
   ```
6. If no PR exists, create one using the template below.

## PR template

```
gh pr create \
--title "<commit message from plan breadcrumb>" \
--body "$(cat <<'EOF'
## Summary
<2-4 bullet points describing what changed and why>

## Files changed
<list each modified file with a one-line description>

## Test plan
{test_plan_items}

Closes #{issue_number}
EOF
)"
```

Do NOT merge the PR.
