#!/bin/sh
# Create an early draft PR after the plan stage for visibility.
# Commits any breadcrumb files, pushes the branch, and creates a draft PR.
# If draft creation fails (e.g., no diff from main), exits 0 so the
# optional stage doesn't block the workflow.

# Stage breadcrumb files if they exist.
git add -A .forza/ .plan_breadcrumb.md 2>/dev/null

# Commit only if there are staged changes.
git diff --cached --quiet || git commit -m "plan: issue #$FORZA_SUBJECT_NUMBER"

# Push the branch.
git push origin HEAD 2>/dev/null

# Read the plan breadcrumb for the PR body.
if [ -f .plan_breadcrumb.md ]; then
    BODY=$(cat .plan_breadcrumb.md)
else
    BODY="Work in progress for issue #$FORZA_SUBJECT_NUMBER"
fi

# Create the draft PR. If it fails (no diff, PR already exists), that's OK.
gh pr create --draft \
    --title "[WIP] issue #$FORZA_SUBJECT_NUMBER" \
    --body "$BODY" \
    2>/dev/null || true
