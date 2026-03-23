# MCP Server

Forza exposes an MCP (Model Context Protocol) server that allows AI agents and MCP-compatible clients to interact with forza programmatically.

## Starting the MCP server

### Stdio transport (default)

```bash
forza mcp [--config path]
```

The server communicates over stdio using the MCP protocol. Connect to it from an MCP client or configure it in an `.mcp.json` file.

### HTTP/SSE transport

```bash
forza mcp --http [--host 127.0.0.1] [--port 8080]
```

Starts an HTTP server with SSE streaming. Useful for remote clients or when stdio is not available.

## Configuring in .mcp.json

Add forza as an MCP server in your project's `.mcp.json`:

```json
{
  "mcpServers": {
    "forza": {
      "command": "forza",
      "args": ["mcp", "--config", "/path/to/forza.toml"]
    }
  }
}
```

## Available tools

Forza exposes 11 tools organized into three groups.

### Runner tools

#### issue_run

Process a single GitHub issue through the full pipeline.

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `number` | integer | yes | Issue number to process |
| `repo` | string | no | `owner/name` (required when multiple repos are configured) |

#### pr_run

Process a single GitHub PR through the full pipeline.

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `number` | integer | yes | PR number to process |
| `repo` | string | no | `owner/name` (required when multiple repos are configured) |

#### run_batch

Poll for all eligible issues and PRs across configured repos and process them in a single batch. Takes no parameters.

#### dry_run_issue

Show the execution plan for an issue without running it.

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `number` | integer | yes | Issue number to show the plan for |
| `repo` | string | no | `owner/name` (required when multiple repos are configured) |

### Status tools

#### status_latest

Get the most recent run record. Takes no parameters.

#### status_list

List all run records sorted newest-first. Takes no parameters.

#### status_get

Get a specific run record by ID.

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `run_id` | string | yes | Run identifier |

#### status_summary

Get per-workflow aggregate statistics across all runs (totals, success/failure counts, cost ranges). Takes no parameters.

#### status_find_issue

Find the most recent run for a given issue number.

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `issue_number` | integer | yes | Issue number to look up |

### Config tools

#### config_show

Return the currently loaded runner configuration as JSON. Takes no parameters.

#### config_validate

Parse and validate a forza config file, returning any errors.

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | yes | Path to the config file to validate |

## Use cases

- **Agent-driven issue processing**: An agent can use `issue_run` to start processing an issue it just created
- **PR maintenance**: Use `pr_run` to trigger pipeline runs for PRs that need rebasing, CI fixes, or review
- **Batch processing**: Use `run_batch` to poll and process all eligible work in one call
- **Dry runs**: Use `dry_run_issue` to preview what forza would do for an issue before committing
- **Status monitoring**: Query run outcomes with `status_list`, `status_latest`, or `status_summary`
- **Integration testing**: Drive forza runs from test harnesses without using the CLI

## Using forza with its own MCP server

Forza uses its own MCP server to process issues. When an agent is working in a forza repository and needs to trigger a run or check status, it can connect to the forza MCP server via the `.mcp.json` in the project root.

See [`forza.toml`](https://github.com/joshrotenberg/forza/blob/main/forza.toml) and [`.mcp.json`](https://github.com/joshrotenberg/forza/blob/main/.mcp.json) in the forza repository for a real-world example.
