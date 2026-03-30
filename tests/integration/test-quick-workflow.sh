#!/usr/bin/env bash
# Integration test: quick workflow end-to-end
#
# Creates a real issue on this repo, runs forza to add a function
# to the test fixture crate, verifies the PR, then cleans up.
#
# Usage:
#   ./tests/integration/test-quick-workflow.sh
#
# Requires: forza binary built, gh CLI authenticated, ANTHROPIC_API_KEY set

set -euo pipefail

REPO="joshrotenberg/forza"
FIXTURE="crates/forza-test-fixture/src/lib.rs"
FORZA="${FORZA:-cargo run -p forza --}"
PASS=0
FAIL=0

pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1"; FAIL=$((FAIL + 1)); }

echo "=== Integration Test: quick workflow ==="
echo "Repo: $REPO"
echo "Fixture: $FIXTURE"
echo ""

# Pick a function that doesn't exist yet
FUNCS=("factorial" "fibonacci" "gcd" "lcm" "min_val" "max_val" "clamp" "sign" "pow_mod" "sum_range" "abs" "is_even" "is_positive" "square" "cube")
FUNC=""
for fn in "${FUNCS[@]}"; do
    if ! grep -q "pub fn $fn" "$FIXTURE" 2>/dev/null; then
        FUNC="$fn"
        break
    fi
done

if [ -z "$FUNC" ]; then
    echo "ERROR: all test functions already exist in $FIXTURE"
    exit 1
fi

echo "Test function: $FUNC"
echo ""

# 1. Create issue
echo "Step 1: Creating issue..."
ISSUE_URL=$(gh issue create --repo "$REPO" \
    --title "[integration-test] add $FUNC to forza-test-fixture calculator" \
    --body "Add a \`$FUNC\` function to the calculator module in \`$FIXTURE\`. Include at least 2 tests. Handle edge cases (zero, negative numbers where applicable). Run \`cargo test -p forza-test-fixture\` to verify." \
    )
ISSUE_NUMBER=$(echo "$ISSUE_URL" | grep -o '[0-9]*$')
echo "  Created issue #$ISSUE_NUMBER"

# Cleanup function
cleanup() {
    echo ""
    echo "Cleaning up..."
    gh issue close "$ISSUE_NUMBER" --repo "$REPO" \
        --comment "Integration test complete." 2>/dev/null || true
    for pr in $(gh pr list --repo "$REPO" --json number,headRefName \
        --jq ".[] | select(.headRefName | contains(\"$ISSUE_NUMBER\")) | .number" 2>/dev/null); do
        gh pr close "$pr" --repo "$REPO" --delete-branch 2>/dev/null || true
    done
    echo ""
    echo "=== Results: $PASS passed, $FAIL failed ==="
    [ "$FAIL" -eq 0 ] && exit 0 || exit 1
}
trap cleanup EXIT

# 2. Run forza
echo ""
echo "Step 2: Running forza..."
OUTPUT=$($FORZA issue "$ISSUE_NUMBER" --workflow quick 2>&1) || true
echo "$OUTPUT" | tail -10

# 3. Verify
echo ""
echo "Step 3: Verifying..."

if echo "$OUTPUT" | grep -q "succeeded"; then
    pass "forza reported success"
else
    fail "forza did not report success"
fi

sleep 3
PR=$(gh pr list --repo "$REPO" --json number,headRefName \
    --jq ".[] | select(.headRefName | contains(\"$ISSUE_NUMBER\")) | .number" \
    | head -1)

if [ -n "$PR" ]; then
    pass "PR #$PR created"
else
    fail "no PR created"
    exit 0
fi

ADDITIONS=$(gh pr view "$PR" --repo "$REPO" --json additions --jq '.additions')
if [ "$ADDITIONS" -gt 0 ] 2>/dev/null; then
    pass "PR has $ADDITIONS additions"
else
    fail "PR has no additions"
fi

BRANCH=$(gh pr view "$PR" --repo "$REPO" --json headRefName --jq '.headRefName')
if gh api "repos/$REPO/contents/$FIXTURE?ref=$BRANCH" --jq '.content' 2>/dev/null | base64 -d 2>/dev/null | grep -q "pub fn $FUNC"; then
    pass "function $FUNC exists in PR"
else
    fail "function $FUNC not found in PR"
fi

echo ""
echo "Done."
