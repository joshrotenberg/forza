#!/bin/sh
# Create an early draft PR for visibility (shell fallback for custom workflows).
# The builtin DraftPr stage uses native execution instead of this script.
# This is kept for custom workflows that define a shell-based draft_pr stage.
set -e

BRANCH=$(git branch --show-current)

# Create an empty commit to establish a diff from main.
git commit --allow-empty -m "wip: $FORZA_SUBJECT_TITLE (#$FORZA_SUBJECT_NUMBER) [skip ci]"

# Push the branch.
git push origin HEAD

# Read the plan breadcrumb for the PR body.
if [ -f .plan_breadcrumb.md ]; then
    BODY=$(cat .plan_breadcrumb.md)
else
    BODY="Work in progress for $FORZA_SUBJECT_TITLE (#$FORZA_SUBJECT_NUMBER)"
fi

# Create the draft PR.
gh pr create --draft \
    --repo "$FORZA_REPO" \
    --head "$BRANCH" \
    --title "[WIP] $FORZA_SUBJECT_TITLE (#$FORZA_SUBJECT_NUMBER)" \
    --body "$BODY" \
    2>/dev/null || true
