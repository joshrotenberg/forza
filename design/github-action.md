# GitHub Action for forza

## Summary

A GitHub Action that runs forza in response to repository events, giving users
the same route→workflow→stages pipeline they get with `forza watch` or `forza run`,
but triggered by GitHub Actions instead of a polling loop.

## Motivation

forza currently supports three invocation modes:

- **`forza watch`** — daemon that polls for eligible work
- **`forza run`** — single batch cycle (discover + process)
- **`forza issue` / `forza pr`** — targeted single-subject execution

A GitHub Action adds a fourth mode: **event-driven execution inside Actions
runners**. This is compelling because:

1. No infrastructure to manage (no EC2, no laptop running overnight)
2. GitHub events replace polling — instant response to labels, PR updates, etc.
3. Same `forza.toml` config drives behavior regardless of where forza runs
4. Composable with other Actions (setup steps, notifications, etc.)

## Design

### Action interface

```yaml
# action.yml
name: "forza"
description: "Run forza to process GitHub issues and PRs through configurable workflows"

inputs:
  version:
    description: "forza version to install (e.g. '0.2.0', 'latest')"
    default: "latest"
  config:
    description: "Path to forza config file"
    default: "forza.toml"
  command:
    description: "forza command to run: 'auto', 'run', 'issue', or 'pr'"
    default: "auto"
  args:
    description: "Additional arguments passed to the forza command"
    default: ""
  anthropic_api_key:
    description: "Anthropic API key (alternative to setting ANTHROPIC_API_KEY env var)"
    required: false
```

### The `auto` command

When `command: auto` (the default), the action inspects the GitHub event context
and determines the right forza command:

| Event | Action trigger | forza command |
|-------|---------------|---------------|
| `issues.labeled` | Label added to issue | `forza issue <number>` |
| `pull_request.labeled` | Label added to PR | `forza pr <number>` |
| `pull_request.synchronize` | PR updated (push) | `forza run` |
| `check_suite.completed` | CI finished | `forza run` |
| `schedule` | Cron trigger | `forza run` |
| Any other | — | `forza run` |

For label events, `auto` extracts the issue/PR number from the event payload and
runs the targeted command. For everything else, it falls back to `forza run` which
does its own discovery.

This means a single workflow file handles all event types:

```yaml
on:
  issues:
    types: [labeled]
  pull_request:
    types: [labeled, synchronize]
  check_suite:
    types: [completed]

jobs:
  forza:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: anthropics/setup-claude-code@v1
      - uses: joshrotenberg/forza-action@v1
        env:
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
```

### Direct command mode

For users who want explicit control:

```yaml
# Only process issue #42
- uses: joshrotenberg/forza-action@v1
  with:
    command: issue
    args: "42 --model claude-opus-4-6"

# Run a full batch cycle
- uses: joshrotenberg/forza-action@v1
  with:
    command: run
    args: "--no-gate"

# Process the PR that triggered this workflow
- uses: joshrotenberg/forza-action@v1
  with:
    command: pr
    args: "${{ github.event.pull_request.number }}"
```

### Flexible `run` for condition routes

`forza run` is the most flexible entry point for Actions. It does full discovery:
gate-labeled issues, label-routed PRs, and condition-routed PRs. This means
condition routes (`auto-rebase`, `auto-fix-ci`, `auto-merge`) work naturally —
`forza run` evaluates all open PRs against conditions and processes matches.

For a `check_suite.completed` trigger, this is ideal: CI just finished, forza
discovers which PRs now match `ci_green_no_objections` or `ci_failing`, and
processes them. No need to map the event to a specific PR — discovery handles it.

### Installation

The action installs forza from GitHub Releases:

```bash
# Detect platform
ARCH=$(uname -m)
OS=$(uname -s | tr '[:upper:]' '[:lower:]')

# Download and install
curl -sL "https://github.com/joshrotenberg/forza/releases/download/v${VERSION}/forza-${OS}-${ARCH}.tar.gz" \
  | tar xz -C /usr/local/bin
```

For `version: latest`, fetch the latest release tag via the GitHub API.

### Gate label behavior

In watch/run mode, forza requires a gate label (`forza:ready`) on issues. In
Actions, this is often redundant — the workflow trigger already gates execution.
The action passes `--no-gate` by default when the trigger is a label event (since
the label *is* the gate). For `schedule` and `check_suite` triggers, gate behavior
is preserved.

Users can override this via `args: "--no-gate"` or by removing `gate_label` from
their config.

### Environment

The action sets up the environment that forza expects:

- `GITHUB_TOKEN` — already available in Actions
- `ANTHROPIC_API_KEY` — from input or env
- Working directory — the checked-out repo
- `gh` CLI — pre-installed on GitHub runners
- `git` — pre-installed on GitHub runners

The agent binary (Claude Code, etc.) must be set up by a preceding step. This is
intentional — the action is agent-agnostic and doesn't assume which agent binary
to install.

## File structure

```
action/
├── action.yml          # Action metadata and inputs
└── entrypoint.sh       # Shell script that maps events to forza commands
```

The action is a composite action using a shell script. No Docker, no Node.js
runtime — just bash wrapping forza. This keeps it simple and fast.

## Future considerations

- **Caching**: Cache the forza binary between runs (`actions/cache`) to skip
  download on repeat runs.
- **Outputs**: Expose `run_id`, `outcome`, `pr_number` as action outputs for
  downstream steps.
- **Matrix builds**: Run forza across multiple repos in a matrix strategy.
- **Reusable workflow**: Provide a reusable workflow (`.github/workflows/forza.yml`)
  that users can call with `workflow_call`.
