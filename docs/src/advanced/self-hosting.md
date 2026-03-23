# Self-Hosting

Running forza as a persistent service that watches GitHub and processes issues and PRs continuously.

## Requirements

- Rust toolchain (for building from source) or a pre-built binary
- `gh` CLI authenticated with appropriate repository permissions
- Claude API key (or Codex credentials) set in the environment
- Git configured with a user name and email

## Running forza watch

The simplest self-hosting setup is `forza watch` as a long-running process:

```bash
# Continuous polling loop, 60-second interval
forza watch --interval 60
```

Forza polls GitHub on the configured `poll_interval` for each route, discovers eligible subjects, and processes them concurrently up to `max_concurrency`.

## Process management

Use a process manager to keep forza running and restart it on failure.

### systemd (Linux)

```ini
[Unit]
Description=forza GitHub automation runner
After=network.target

[Service]
Type=simple
User=forza
WorkingDirectory=/opt/forza
ExecStart=/usr/local/bin/forza watch --config /etc/forza/forza.toml
Restart=on-failure
RestartSec=30
Environment=ANTHROPIC_API_KEY=sk-...

[Install]
WantedBy=multi-user.target
```

Enable and start:

```bash
systemctl enable forza
systemctl start forza
```

### Docker

```dockerfile
FROM rust:1.90-slim AS builder
WORKDIR /build
RUN cargo install forza

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y git gh && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/cargo/bin/forza /usr/local/bin/forza
WORKDIR /workspace
ENTRYPOINT ["forza", "watch"]
```

## Environment variables

| Variable | Description |
|----------|-------------|
| `ANTHROPIC_API_KEY` | API key for Claude (required when `agent = "claude"`) |
| `OPENAI_API_KEY` | API key for Codex (required when `agent = "codex"`) |
| `GH_TOKEN` | GitHub token (alternative to `gh auth login`) |

## Cost management

Use cost guardrails to bound spending:

```toml
[global]
max_cost_per_issue = 5.00    # Stop a single run if it exceeds $5
max_cost_per_hour = 20.00    # Pause all routes if hourly spend exceeds $20
```

Monitor costs with `forza status`:

```bash
forza status --limit 50
```

## Real-world example

The forza repository uses forza to process its own issues. See [`forza.toml`](https://github.com/joshrotenberg/forza/blob/main/forza.toml) in the repository for a complete, production configuration.
