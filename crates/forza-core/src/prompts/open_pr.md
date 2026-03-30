{preamble}

Create or update a pull request for issue #{issue_number}.

## Steps

1. Push the branch to origin: `git push origin HEAD`
2. Use the context from previous stages (provided above) for the commit message, files changed, and review verdict.
3. Check if a draft PR already exists on this branch: `gh pr list --head $(git branch --show-current) --json number,isDraft --jq '.[0]'`
4. If a draft PR exists, update it and mark it ready for review:
   ```
   gh pr edit <number> --title "<commit message>" --body "<PR body>"
   gh pr ready <number>
   ```
5. If no PR exists, create one using the template below.

## PR template

```
gh pr create \
--title "<commit message from context above>" \
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
