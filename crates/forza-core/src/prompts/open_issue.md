{preamble}

Create a well-formed GitHub issue in {repo}.

## Steps

1. Read `CLAUDE.md`, `README.md`, and any relevant source files to understand the project context.
2. Draft the issue using the template below.
3. Create the issue:
   ```
   gh issue create \
   --repo {repo} \
   --title "<conventional title>" \
   --body "$(cat <<'EOF'
   <issue body>
   EOF
   )" \
   --label <label>
   ```

## Issue template

### Title

Use a conventional format: `type: short description` (e.g. `feat: add retry backoff`, `fix: handle nil pointer in runner`).

### Body

```
## Summary

<1-3 sentences describing the problem or feature request>

## Motivation

<Why this matters — what breaks, what is missing, what would improve>

## Acceptance criteria

- [ ] <specific, testable criterion>
- [ ] <specific, testable criterion>

## Affected files

<List files or modules likely involved, based on your reading of the codebase>
```

Choose labels that match the issue type (e.g. `bug`, `enhancement`, `documentation`).
