# REST API

Forza exposes a REST API for programmatic control and status inspection.

## Starting the API server

```bash
forza serve [--port <port>] [--config path]
```

Default port is `8080`. The server binds to `127.0.0.1` by default.

## Endpoints

### GET /health

Returns the server health status.

**Response:**

```json
{ "status": "ok" }
```

### GET /runs

List recent runs.

**Response:**

```json
[
  {
    "id": "run-abc123",
    "route": "bugfix",
    "workflow": "bug",
    "subject_number": 42,
    "subject_type": "issue",
    "outcome": "PrCreated",
    "started_at": "2025-01-01T00:00:00Z",
    "finished_at": "2025-01-01T00:05:00Z"
  }
]
```

### GET /runs/latest

Get the most recent run.

### GET /runs/{run_id}

Get details for a specific run.

### POST /runs/issue/{number}

Trigger a run for a specific issue.

**Query parameters:**

| Parameter | Description |
|-----------|-------------|
| `repo` | Target repo (required when multiple repos are configured) |
| `dry_run` | If `true`, return the matched route and plan without executing |

### POST /runs/pr/{number}

Trigger a run for a specific pull request.

**Query parameters:**

| Parameter | Description |
|-----------|-------------|
| `repo` | Target repo (required when multiple repos are configured) |
| `dry_run` | If `true`, return the matched route and plan without executing |

### POST /runs/batch

Trigger a full batch cycle across all configured repos.

### GET /status

Return current runner status (workflow summaries with run counts and cost stats).

### GET /config

Return the current runner configuration.

## Authentication

The API server does not currently implement authentication. Bind it to `localhost` only and do not expose it publicly.
