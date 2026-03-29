#!/bin/sh
# Create an early draft PR for visibility.
# Pushes the branch and creates a draft PR.
set -x

# Create an empty commit to establish a diff from main.
git commit --allow-empty -m "wip: $FORZA_SUBJECT_TITLE (#$FORZA_SUBJECT_NUMBER) [skip ci]"

# Push the branch.
git push origin HEAD

# Check gh auth status
gh auth status

# Read the plan breadcrumb for the PR body.
if [ -f .plan_breadcrumb.md ]; then
    BODY=$(cat .plan_breadcrumb.md)
else
    BODY="Work in progress for $FORZA_SUBJECT_TITLE (#$FORZA_SUBJECT_NUMBER)"
fi

# Create the draft PR.
gh pr create --draft \
    --title "[WIP] $FORZA_SUBJECT_TITLE (#$FORZA_SUBJECT_NUMBER)" \
    --body "$BODY"
