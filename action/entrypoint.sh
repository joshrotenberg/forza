#!/usr/bin/env bash
set -euo pipefail

# ── Resolve the forza command ────────────────────────────────────────────
#
# In "auto" mode, inspect the GitHub event to determine the right command.
# Label events → targeted issue/pr command. Everything else → run (discovery).

COMMAND="${FORZA_COMMAND}"
ARGS="${FORZA_ARGS}"
CONFIG_FLAG="--config ${FORZA_CONFIG}"
NO_GATE=""

if [ "$COMMAND" = "auto" ]; then
  case "${GITHUB_EVENT_NAME}" in
    issues)
      if [ -n "${EVENT_ISSUE_NUMBER:-}" ]; then
        COMMAND="issue"
        ARGS="${EVENT_ISSUE_NUMBER} ${ARGS}"
        NO_GATE="--no-gate"  # label event is the gate
      else
        COMMAND="run"
      fi
      ;;
    pull_request|pull_request_target)
      if [ -n "${EVENT_PR_NUMBER:-}" ]; then
        COMMAND="pr"
        ARGS="${EVENT_PR_NUMBER} ${ARGS}"
      else
        COMMAND="run"
      fi
      ;;
    check_suite|check_run|workflow_run)
      # CI finished — let forza discover which PRs need attention.
      COMMAND="run"
      ;;
    schedule)
      # Cron trigger — full discovery cycle.
      COMMAND="run"
      ;;
    *)
      # Unknown event — fall back to run.
      COMMAND="run"
      ;;
  esac
fi

# For "run" command, skip gate label by default in Actions context since
# the workflow trigger already gates execution. Users can override in config.
if [ "$COMMAND" = "run" ] && [ -z "$NO_GATE" ]; then
  NO_GATE="--no-gate"
fi

# ── Execute ──────────────────────────────────────────────────────────────

echo "::group::forza ${COMMAND}"
echo "Running: forza ${CONFIG_FLAG} ${COMMAND} ${NO_GATE} ${ARGS}"

# shellcheck disable=SC2086
forza ${CONFIG_FLAG} ${COMMAND} ${NO_GATE} ${ARGS} 2>&1 | tee /tmp/forza-output.log
EXIT_CODE=${PIPESTATUS[0]}

echo "::endgroup::"

# ── Parse outputs ────────────────────────────────────────────────────────

# Extract run ID and outcome from forza output if available.
RUN_ID=$(grep -oP 'run_id:\s*\K\S+' /tmp/forza-output.log 2>/dev/null || echo "")
OUTCOME=$(grep -oP 'outcome:\s*\K.*' /tmp/forza-output.log 2>/dev/null || echo "")

if [ -n "$RUN_ID" ]; then
  echo "run-id=${RUN_ID}" >> "$GITHUB_OUTPUT"
fi
if [ -n "$OUTCOME" ]; then
  echo "outcome=${OUTCOME}" >> "$GITHUB_OUTPUT"
fi

exit "$EXIT_CODE"
