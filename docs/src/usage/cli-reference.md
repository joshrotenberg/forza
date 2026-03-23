# CLI Reference

## Commands

### forza init

Create the required GitHub labels and generate a starter `forza.toml`:

```
forza init [--repo owner/name] [--config path]
```

This is a one-time setup command. It is idempotent — safe to run on a repo that already has some of the labels.

### forza issue

Process a single issue by number:

```
forza issue <N> [--dry-run] [--config path]
```

Fetches the issue, matches a route, and executes the workflow. Use `--dry-run` to preview the match and planned stages without executing.

### forza pr

Process a single PR by number:

```
forza pr <N> [--dry-run] [--config path]
```

Fetches the PR, matches a condition or label route, and executes the workflow.

### forza run

Discover and process one batch of eligible issues and PRs:

```
forza run [--config path]
```

Equivalent to one iteration of the watch loop. Useful for running forza from a cron job or CI.

### forza watch

Continuous polling loop with auto-fix:

```
forza watch [--interval <seconds>] [--config path]
```

Runs `forza run` on the configured `poll_interval` for each route, indefinitely. Use `Ctrl+C` to stop.

### forza status

Show run history and outcomes:

```
forza status [--limit N] [--config path]
```

Displays recent runs with their route, workflow, outcome, and cost.

### forza explain

Visualize your config, routes, and workflows:

```
forza explain [--issues] [--prs] [--conditions]
              [--route <name>] [--workflows] [--workflow <name>]
              [-v] [--json] [--config path]
```

Useful for verifying that routes are configured correctly before running.

### forza fix

Re-run failed stages with error context:

```
forza fix [--run-id <id>] [--config path]
```

Picks up where a failed run left off, passing the failure reason as additional context to the agent.

### forza clean

Remove worktrees and run state:

```
forza clean [--all] [--config path]
```

Removes stale git worktrees created by forza and cleans up local run state. Use `--all` to remove all forza-created worktrees, including ones from active runs.

### forza serve

Start the REST API server:

```
forza serve [--port <port>] [--config path]
```

Starts an HTTP server exposing the forza API. See [REST API](../advanced/api.md) for endpoint documentation.

### forza mcp

Start the MCP server:

```
forza mcp [--config path]
```

Starts an MCP server on stdio. See [MCP Server](../advanced/mcp.md) for tool documentation.

## Global flags

| Flag | Description |
|------|-------------|
| `--config <path>` | Path to `forza.toml` (default: `./forza.toml`) |
| `--dry-run` | Preview actions without executing (supported on `issue`, `pr`) |
| `--verbose` / `-v` | Increase log verbosity |
