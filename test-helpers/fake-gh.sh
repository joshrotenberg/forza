#!/usr/bin/env bash
# Fake gh CLI for forza integration tests.
#
# Handles the subset of gh commands that forza uses.
# Controlled by environment variables:
#   FAKE_GH_ISSUE_JSON     - JSON response for `gh issue view`
#   FAKE_GH_PR_CREATE_URL  - URL returned by `gh pr create`
#   FAKE_GH_PR_MERGE_FAIL  - if "true", `gh pr merge` fails
#   FAKE_GH_LOG            - if set, append commands to this file for assertions
#
# Labels are tracked in $FAKE_GH_STATE_DIR/labels-{number}.txt

STATE_DIR="${FAKE_GH_STATE_DIR:-/tmp/fake-gh-state}"
mkdir -p "$STATE_DIR"

# Log the command for test assertions.
if [[ -n "$FAKE_GH_LOG" ]]; then
    echo "$*" >> "$FAKE_GH_LOG"
fi

case "$1" in
    issue)
        case "$2" in
            view)
                # Return canned issue JSON.
                if [[ -n "$FAKE_GH_ISSUE_JSON" ]]; then
                    echo "$FAKE_GH_ISSUE_JSON"
                else
                    cat <<'JSON'
{"number":1,"title":"test issue","body":"test body","labels":[{"name":"bug"},{"name":"forza:ready"}],"state":"open","createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z","assignees":[],"url":"https://github.com/test/repo/issues/1","comments":[],"author":{"login":"testuser"}}
JSON
                fi
                ;;
            list)
                # Return empty list by default.
                echo "[]"
                ;;
            edit)
                # Track label changes.
                number=""
                for arg in "$@"; do
                    if [[ "$arg" =~ ^[0-9]+$ ]]; then
                        number="$arg"
                    fi
                done
                shift 2  # remove "issue edit"
                while [[ $# -gt 0 ]]; do
                    case "$1" in
                        --add-label)
                            echo "$2" >> "$STATE_DIR/labels-$number.txt"
                            shift 2
                            ;;
                        --remove-label)
                            if [[ -f "$STATE_DIR/labels-$number.txt" ]]; then
                                grep -v "^$2$" "$STATE_DIR/labels-$number.txt" > "$STATE_DIR/labels-$number.tmp" 2>/dev/null
                                mv "$STATE_DIR/labels-$number.tmp" "$STATE_DIR/labels-$number.txt"
                            fi
                            shift 2
                            ;;
                        *)
                            shift
                            ;;
                    esac
                done
                ;;
            comment)
                # Record comment.
                number=""
                body=""
                for arg in "$@"; do
                    if [[ "$arg" =~ ^[0-9]+$ ]]; then
                        number="$arg"
                    fi
                    if [[ "$prev" == "--body" ]]; then
                        body="$arg"
                    fi
                    prev="$arg"
                done
                echo "$body" >> "$STATE_DIR/comments-$number.txt"
                ;;
        esac
        ;;
    pr)
        case "$2" in
            create)
                echo "${FAKE_GH_PR_CREATE_URL:-https://github.com/test/repo/pull/99}"
                ;;
            merge)
                if [[ "$FAKE_GH_PR_MERGE_FAIL" == "true" ]]; then
                    echo "merge failed" >&2
                    exit 1
                fi
                ;;
            view)
                echo '{"number":99,"title":"test PR","body":"","labels":[],"state":"open","url":"https://github.com/test/repo/pull/99","headRefName":"automation/1-test","baseRefName":"main","isDraft":false,"mergeable":"MERGEABLE","reviewDecision":null,"statusCheckRollup":[]}'
                ;;
            list)
                echo "[]"
                ;;
            comment)
                number=""
                body=""
                for arg in "$@"; do
                    if [[ "$arg" =~ ^[0-9]+$ ]]; then
                        number="$arg"
                    fi
                    if [[ "$prev" == "--body" ]]; then
                        body="$arg"
                    fi
                    prev="$arg"
                done
                echo "$body" >> "$STATE_DIR/pr-comments-$number.txt"
                ;;
            checks)
                # Return success by default.
                echo "All checks passed"
                ;;
        esac
        ;;
    label)
        # Label creation — no-op.
        ;;
    *)
        echo "fake-gh: unknown command: $*" >&2
        exit 1
        ;;
esac

exit 0
