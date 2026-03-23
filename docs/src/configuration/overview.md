# Configuration

Forza is configured via `forza.toml` in the directory where you run the forza process. The file uses TOML with named sections for global settings, security, validation, repos, and workflow templates.

## Structure

```toml
[global]           # Global settings: model, gate_label, concurrency
[security]         # Authorization level, allowed authors
[validation]       # Commands run between stages
[agent_config]     # Agent skill files, MCP config, system prompt
[stage_hooks.*]    # Pre/post/finally hooks per stage kind
[repos."owner/name"]             # Per-repo settings
[repos."owner/name".routes.*]    # Named routes per repo
[[workflow_templates]]           # Custom workflow templates
```

## Configuration file location

Forza looks for `forza.toml` in the current directory by default. Use `--config` to specify a different path:

```bash
forza run --config /path/to/forza.toml
```

## Sections

### [global]

Settings that apply to all repos unless overridden per route:

```toml
[global]
model = "claude-sonnet-4-6"    # Default agent model
agent = "claude"               # Agent backend: "claude" or "codex"
gate_label = "forza:ready"     # Label gate; omit to process all matching routes
branch_pattern = "automation/{issue}-{slug}"
max_concurrency = 5            # Total parallel runs across all routes
max_cost_per_issue = 5.00      # Cost cap per issue (USD)
max_cost_per_hour = 20.00      # Hourly cost cap (USD)
```

### [security]

Controls what the agent is authorized to do and who can trigger runs:

```toml
[security]
authorization_level = "contributor"   # sandbox | local | contributor | trusted
allowed_authors = []                  # Empty = authenticated user only
require_label = "security:approved"   # Additional approval gate (optional)
```

### [validation]

Commands that run between every stage. All must pass before the next stage starts:

```toml
[validation]
commands = [
    "cargo fmt --all -- --check",
    "cargo clippy --all-targets -- -D warnings",
    "cargo test --lib",
]
```

### [agent_config]

Customize the agent invocation:

```toml
[agent_config]
skills = ["./skills/rust.md"]           # Skill files prepended to prompts
mcp_config = ".mcp.json"                # MCP server config path
append_system_prompt = "..."            # Extra instructions appended to system prompt
```

### [stage_hooks.*]

Shell commands to run before, after, or regardless of stage outcome:

```toml
[stage_hooks.implement]
pre     = ["echo 'starting implement'"]
post    = ["cargo fmt --all"]
finally = ["echo 'implement done'"]
```

### [repos."owner/name"]

Per-repo configuration. The repo key is `owner/name`:

```toml
[repos."your-org/your-repo"]
repo_dir = "/path/to/local/checkout"   # Optional; defaults to current directory
```

### [repos."owner/name".routes.*]

Named routes for a repo. See [Routes](../concepts/routes.md) for full field documentation.

### [[workflow_templates]]

Custom workflow templates. See [Workflows](../concepts/workflows.md) for details.

## Next steps

- [Configuration Reference](reference.md) — full field documentation
- [Examples](examples.md) — ready-to-use configurations
