# REST API

Forza exposes a REST API for programmatic control and status inspection.

## Starting the API server

```bash
forza serve [--port <port>] [--config path]
```

Default port is `3000`. The server binds to `127.0.0.1` by default.

## Endpoints

### GET /health

Returns the server health status.

**Response:**

```json
{ "status": "ok" }
```

### GET /runs

List recent runs.

**Query parameters:**

| Parameter | Description |
|-----------|-------------|
| `limit` | Maximum number of runs to return (default: 20) |
| `route` | Filter by route name |
| `outcome` | Filter by outcome |

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

### GET /runs/:id

Get details for a specific run.

### POST /runs

Trigger a run manually.

**Request body:**

```json
{
  "repo": "owner/name",
  "subject_type": "issue",
  "subject_number": 42,
  "route": "bugfix"
}
```

### GET /routes

List all configured routes.

### GET /status

Return current runner status (active runs, queue depth, hourly cost).

## Authentication

The API server does not currently implement authentication. Bind it to `localhost` only and do not expose it publicly.
