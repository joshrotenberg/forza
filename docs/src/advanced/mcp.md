# MCP Server

Forza exposes an MCP (Model Context Protocol) server that allows AI agents and MCP-compatible clients to interact with forza programmatically.

## Starting the MCP server

```bash
forza mcp [--config path]
```

The server communicates over stdio using the MCP protocol. Connect to it from an MCP client or configure it in an `.mcp.json` file.

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

### list_routes

List all configured routes and their current status.

### list_runs

List recent runs with outcomes and metadata.

**Parameters:**

| Parameter | Type | Description |
|-----------|------|-------------|
| `limit` | integer | Maximum number of runs to return |
| `route` | string | Filter by route name |

### trigger_run

Trigger a run for a specific issue or PR.

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `repo` | string | yes | `owner/name` |
| `subject_type` | string | yes | `"issue"` or `"pr"` |
| `subject_number` | integer | yes | Issue or PR number |
| `route` | string | no | Route name (inferred from config if omitted) |

### get_run

Get detailed information about a specific run by ID.

**Parameters:**

| Parameter | Type | Description |
|-----------|------|-------------|
| `run_id` | string | Run identifier |

### get_status

Get the current runner status: active runs, queue depth, and cost.

## Use cases

- **Agent-driven issue creation**: An agent can use `trigger_run` to start processing an issue it just created
- **Status monitoring**: Query run outcomes from an automated dashboard
- **Integration testing**: Drive forza runs from test harnesses without using the CLI

## Using forza with its own MCP server

Forza uses its own MCP server to process issues. When an agent is working in a forza repository and needs to trigger a run or check status, it can connect to the forza MCP server via the `.mcp.json` in the project root.

See [`forza.toml`](https://github.com/joshrotenberg/forza/blob/main/forza.toml) and [`.mcp.json`](https://github.com/joshrotenberg/forza/blob/main/.mcp.json) in the forza repository for a real-world example.
