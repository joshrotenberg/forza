#!/bin/sh
# Create an early draft PR after the plan stage for visibility.
# Creates an empty commit so the branch has a diff from main, pushes,
# and creates a draft PR with the plan breadcrumb as the body.
# If draft creation fails, exits 0 so the optional stage doesn't block.

# Create an empty commit to establish a diff from main.
git commit --allow-empty -m "wip: $FORZA_SUBJECT_TITLE (#$FORZA_SUBJECT_NUMBER) [skip ci]" 2>/dev/null

# Push the branch.
git push origin HEAD 2>&1 || echo "warning: git push failed" >&2

# Read the plan breadcrumb for the PR body.
if [ -f .plan_breadcrumb.md ]; then
    BODY=$(cat .plan_breadcrumb.md)
else
    BODY="Work in progress for $FORZA_SUBJECT_TITLE (#$FORZA_SUBJECT_NUMBER)"
fi

# Create the draft PR. If it fails (PR already exists, etc.), that's OK.
gh pr create --draft \
    --title "[WIP] $FORZA_SUBJECT_TITLE (#$FORZA_SUBJECT_NUMBER)" \
    --body "$BODY" \
    2>&1 || echo "warning: gh pr create failed" >&2
