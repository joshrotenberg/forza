#!/usr/bin/env bash
# forza GitHub Action entrypoint
#
# Maps GitHub events to forza commands when command is "auto".
# For explicit commands, passes through directly.
set -euo pipefail

COMMAND="${FORZA_COMMAND}"
ARGS="${FORZA_ARGS}"

# Auto mode: inspect the GitHub event and determine the right command.
if [ "$COMMAND" = "auto" ]; then
  case "${GITHUB_EVENT_NAME}" in
    issues)
      if [ -n "${ISSUE_NUMBER:-}" ]; then
        echo "Event: issues.${GITHUB_EVENT_ACTION} on #${ISSUE_NUMBER}"
        COMMAND="issue"
        ARGS="${ISSUE_NUMBER} --no-gate ${ARGS}"
      else
        echo "Event: issues (no number), falling back to run"
        COMMAND="run"
      fi
      ;;
    pull_request)
      if [ -n "${PR_NUMBER:-}" ]; then
        echo "Event: pull_request.${GITHUB_EVENT_ACTION} on #${PR_NUMBER}"
        COMMAND="pr"
        ARGS="${PR_NUMBER} ${ARGS}"
      else
        echo "Event: pull_request (no number), falling back to run"
        COMMAND="run"
      fi
      ;;
    check_suite|check_run)
      echo "Event: ${GITHUB_EVENT_NAME}.${GITHUB_EVENT_ACTION}, running discovery"
      COMMAND="run"
      ;;
    schedule)
      echo "Event: schedule, running discovery"
      COMMAND="run"
      ;;
    workflow_dispatch)
      echo "Event: workflow_dispatch, running discovery"
      COMMAND="run"
      ;;
    *)
      echo "Event: ${GITHUB_EVENT_NAME}, running discovery"
      COMMAND="run"
      ;;
  esac
fi

# Build the full command. Config flag goes after args since positional args
# (like issue number) must come right after the subcommand.
CMD="forza ${COMMAND} ${ARGS} --config ${FORZA_CONFIG}"
echo "Running: ${CMD}"

# Capture output for action outputs.
OUTPUT=$(eval "${CMD}" 2>&1) || EXIT_CODE=$?
EXIT_CODE=${EXIT_CODE:-0}

echo "$OUTPUT"

# Parse outcome and run_id from output if available.
OUTCOME=$(echo "$OUTPUT" | grep -oP 'Outcome:\s+\K\S+' || echo "unknown")
RUN_ID=$(echo "$OUTPUT" | grep -oP 'Run \K[^\s]+' || echo "unknown")

echo "outcome=${OUTCOME}" >> "$GITHUB_OUTPUT"
echo "run_id=${RUN_ID}" >> "$GITHUB_OUTPUT"

exit $EXIT_CODE
