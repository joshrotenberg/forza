#!/bin/sh
# Wait for all CI checks to pass, then merge the PR.
# Uses --watch so the command blocks until checks complete or fail.
# If any check fails, gh pr checks exits non-zero and the merge is skipped.
# The && ensures we only merge if checks pass.

set -e
gh pr checks "$FORZA_PR_NUMBER" --watch && gh pr merge "$FORZA_PR_NUMBER" --squash
