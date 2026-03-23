# CLI Reference

## Commands

### forza init

Create the required GitHub labels and generate a starter `forza.toml`:

```
forza init --repo <owner/name> [--output <path>] [--auto] [--guided] [--model <model>] [--config <path>]
```

| Flag | Description |
|------|-------------|
| `--repo <owner/name>` | Repository in owner/name format (required) |
| `--output <path>` | Output path for the generated config file (default: `forza.toml`) |
| `--auto` | Use an agent to inspect the repo and generate a tailored config |
| `--guided` | Launch an interactive Claude session to collaboratively generate a config |
| `--model <model>` | Model to use for agent-assisted config generation (e.g. `claude-opus-4-6`) |

This is a one-time setup command. It is idempotent — safe to run on a repo that already has some of the labels.

### forza issue

Process a single issue by number:

```
forza issue <N> [--repo <owner/name>] [--repo-dir <path>] [--dry-run] [--model <model>] [--skill <path>...] [--config <path>]
```

| Flag | Description |
|------|-------------|
| `--repo <owner/name>` | Repository to process (required when multiple repos are configured) |
| `--repo-dir <path>` | Repository directory (default: current directory) |
| `--dry-run` | Show the plan without executing |
| `--model <model>` | Override the model for every stage in this run |
| `--skill <path>` | Add a skill file for every stage in this run (repeatable) |

Fetches the issue, matches a route, and executes the workflow. Use `--dry-run` to preview the match and planned stages without executing.

### forza pr

Process a single PR by number:

```
forza pr <N> [--repo <owner/name>] [--repo-dir <path>] [--dry-run] [--model <model>] [--skill <path>...] [--config <path>]
```

| Flag | Description |
|------|-------------|
| `--repo <owner/name>` | Repository to process (required when multiple repos are configured) |
| `--repo-dir <path>` | Repository directory (default: current directory) |
| `--dry-run` | Show the plan without executing |
| `--model <model>` | Override the model for every stage in this run |
| `--skill <path>` | Add a skill file for every stage in this run (repeatable) |

Fetches the PR, matches a condition or label route, and executes the workflow.

### forza run

Discover and process one batch of eligible issues and PRs:

```
forza run [--route <name>] [--repo-dir <path>] [--no-gate] [--config <path>]
```

| Flag | Description |
|------|-------------|
| `--route <name>` | Only run a specific route |
| `--repo-dir <path>` | Repository directory (default: current directory) |
| `--no-gate` | Bypass the gate_label requirement and process all matching issues immediately |

Equivalent to one iteration of the watch loop. Useful for running forza from a cron job or CI.

### forza watch

Continuous polling loop with auto-fix:

```
forza watch [--interval <seconds>] [--route <name>] [--repo-dir <path>]
            [--serve-api] [--api-host <host>] [--api-port <port>]
            [--no-gate] [--config <path>]
```

| Flag | Description |
|------|-------------|
| `--interval <seconds>` | Override poll interval in seconds (uses per-route intervals by default) |
| `--route <name>` | Only run a specific route |
| `--repo-dir <path>` | Repository directory |
| `--serve-api` | Also start the REST API server alongside the watch loop |
| `--api-host <host>` | Host address for the REST API server (default: `127.0.0.1`) |
| `--api-port <port>` | Port for the REST API server (default: `8080`) |
| `--no-gate` | Bypass the gate_label requirement and process all matching issues immediately |

Runs `forza run` on the configured `poll_interval` for each route, indefinitely. Use `Ctrl+C` to stop.

### forza status

Show run history and outcomes:

```
forza status [--all] [--detailed] [--run-id <id>] [--workflow <name>] [--config <path>]
```

| Flag | Description |
|------|-------------|
| `--all` | Show all runs as a history table (sorted newest first) |
| `--detailed` | Show latest run detail |
| `--run-id <id>` | Show a specific run by ID |
| `--workflow <name>` | Filter dashboard to a single workflow |

Displays recent runs with their route, workflow, outcome, and cost.

### forza explain

Visualize your config, routes, and workflows:

```
forza explain [--repo <owner/name>] [--issues] [--prs] [--conditions]
              [--route <name>] [--workflows] [--workflow <name>]
              [-v] [--json] [--config path]
```

| Flag | Description |
|------|-------------|
| `--repo <owner/name>` | Filter output to a single repository |
| `--issues` | Show only issue routes |
| `--prs` | Show only PR routes (label and condition) |
| `--conditions` | Show only condition routes |
| `--route <name>` | Show a single route in detail (auto-verbose) |
| `--workflows` | List all workflow templates |
| `--workflow <name>` | Show a single workflow's stages |
| `-v` / `--verbose` | Verbose output — show per-stage detail |
| `--json` | Output as JSON instead of human-readable text |

Useful for verifying that routes are configured correctly before running.

### forza fix

Re-run failed stages with error context:

```
forza fix [--run <id>] [--issue <N>] [--config <path>]
```

| Flag | Description |
|------|-------------|
| `--run <id>` | Run ID to fix (default: latest run) |
| `--issue <N>` | Issue number to fix (finds latest run for this issue) |

Picks up where a failed run left off, passing the failure reason as additional context to the agent.

### forza clean

Remove worktrees and run state:

```
forza clean [--repo-dir <path>] [--runs] [--stale] [--days <N>] [--dry-run] [--config <path>]
```

| Flag | Description |
|------|-------------|
| `--repo-dir <path>` | Repository directory (default: current directory) |
| `--runs` | Remove run state files instead of worktrees |
| `--stale` | Remove only worktrees older than the configured threshold (see `--days`) |
| `--days <N>` | Age threshold in days for `--stale` (overrides the configured default) |
| `--dry-run` | Print what would be removed without acting |

Removes stale git worktrees created by forza and cleans up local run state.

### forza serve

Start the REST API server:

```
forza serve [--host <host>] [--port <port>] [--repo-dir <path>] [--config <path>]
```

| Flag | Description |
|------|-------------|
| `--host <host>` | Host address to bind to (default: `127.0.0.1`) |
| `--port <port>` | Port to listen on (default: `8080`) |
| `--repo-dir <path>` | Repository directory |

Starts an HTTP server exposing the forza API. See [REST API](../advanced/api.md) for endpoint documentation.

### forza mcp

Start the MCP server:

```
forza mcp [--http] [--host <host>] [--port <port>] [--config <path>]
```

| Flag | Description |
|------|-------------|
| `--http` | Use HTTP/SSE transport instead of stdio |
| `--host <host>` | Host address to bind to, HTTP mode only (default: `127.0.0.1`) |
| `--port <port>` | Port to listen on, HTTP mode only (default: `8080`) |

Starts an MCP server on stdio. See [MCP Server](../advanced/mcp.md) for tool documentation.

### forza open

Open a new GitHub issue using agent assistance:

```
forza open [--repo <owner/name>] [--prompt <text>] [--label <label>] [--ready] [--model <model>] [--config <path>]
```

| Flag | Description |
|------|-------------|
| `--repo <owner/name>` | Repository to open an issue in (required when multiple repos are configured) |
| `--prompt <text>` | Prompt describing the issue to open |
| `--label <label>` | Label to apply to the created issue |
| `--ready` | Also add the `forza:ready` label to the created issue |
| `--model <model>` | Override the model (e.g. `claude-opus-4-6`) |

## Global flags

| Flag | Description |
|------|-------------|
| `--config <path>` / `-c` | Path to `forza.toml` (default: `./forza.toml`) |
| `--log-file <path>` | Write tracing output to this file instead of stderr |
| `--dry-run` | Preview actions without executing (supported on `issue`, `pr`) |
