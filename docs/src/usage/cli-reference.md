# CLI Reference

## Commands

### forza init

Create the required GitHub labels and generate a starter `forza.toml`:

```
forza init --repo <owner/name> [--output <path>] [--auto] [--guided] [--model <model>]
```

| Flag | Description |
|------|-------------|
| `--repo <owner/name>` | Repository in owner/name format (required) |
| `--output <path>` | Output path for the generated config file (default: `forza.toml`) |
| `--auto` | Use an agent to inspect the repo and generate a tailored config |
| `--guided` | Launch an interactive Claude session to collaboratively generate a config |
| `--model <model>` | Model to use for agent-assisted config generation |

This is a one-time setup command. It is idempotent — safe to run on a repo that already has some of the labels.

### forza issue

Process a single issue by number:

```
forza issue <N> [--workflow <name>] [--model <model>] [--fix] [--dry-run] [--skill <path>...]
```

| Flag | Description |
|------|-------------|
| `--workflow <name>` | Override the workflow template, skipping route matching (e.g. `feature`, `bug`, `chore`) |
| `--model <model>` | Override the model for every stage in this run |
| `--fix` | Re-run the latest failed run for this issue with error context |
| `--dry-run` | Show the plan without executing |
| `--skill <path>` | Add a skill file for every stage (repeatable) |
| `--repo <owner/name>` | Repository to process (required when multiple repos are configured) |
| `--repo-dir <path>` | Repository directory (default: current directory) |

When `--workflow` is provided, no `forza.toml` is required — forza infers the repo from the git remote and uses the specified workflow directly. Without `--workflow`, forza matches the issue's labels against configured routes.

If the issue has the `forza:plan` label, forza automatically executes it as a plan instead.

### forza pr

Process a single PR by number:

```
forza pr <N> [--workflow <name>] [--model <model>] [--fix] [--dry-run] [--skill <path>...]
```

| Flag | Description |
|------|-------------|
| `--workflow <name>` | Override the workflow template, skipping route matching (e.g. `pr-fix`, `pr-rebase`) |
| `--model <model>` | Override the model for every stage in this run |
| `--fix` | Re-run the latest failed run for this PR with error context |
| `--dry-run` | Show the plan without executing |
| `--skill <path>` | Add a skill file for every stage (repeatable) |
| `--repo <owner/name>` | Repository to process |
| `--repo-dir <path>` | Repository directory (default: current directory) |

Like `forza issue`, the `--workflow` flag enables configless usage for one-off PR operations.

### forza run

Discover and process one batch of eligible issues and PRs:

```
forza run [--watch] [--interval <seconds>] [--route <name>] [--no-gate] [--serve-api]
```

| Flag | Description |
|------|-------------|
| `--watch` | Continuous polling mode — runs repeatedly on the configured poll interval |
| `--interval <seconds>` | Override poll interval in seconds (watch mode only) |
| `--route <name>` | Only run a specific route |
| `--no-gate` | Bypass the gate_label requirement and process all matching issues immediately |
| `--serve-api` | Also start the REST API server alongside the watch loop (watch mode only) |
| `--api-host <host>` | Host for the API server (default: `127.0.0.1`, watch mode only) |
| `--api-port <port>` | Port for the API server (default: `8080`, watch mode only) |
| `--repo-dir <path>` | Repository directory |

Without `--watch`, runs a single discovery and processing cycle. With `--watch`, runs continuously until stopped with `Ctrl+C`. Requires a `forza.toml` with configured routes.

### forza plan

Create, revise, or execute a plan for a set of issues:

```
forza plan [ISSUES...] [--revise <N>] [--exec <N>] [--dry-run] [--close] [--branch <name>]
```

| Flag | Description |
|------|-------------|
| `[ISSUES...]` | Issue numbers to plan: single (`42`), multiple (`10 20 30`), range (`10..20`). Omit for all open issues |
| `--label <label>` | Only plan issues with this label |
| `--revise <N>` | Revise plan issue #N based on human comments |
| `--exec <N>` | Execute plan issue #N in dependency order |
| `--dry-run` | Preview execution order without processing (use with `--exec`) |
| `--close` | Close the plan issue after all items are executed (use with `--exec`) |
| `--branch <name>` | Target a plan branch for all PRs (e.g. `plan/my-feature`) |
| `--model <model>` | Override the model |
| `--limit <N>` | Maximum issues to fetch (default: 50) |
| `--repo <owner/name>` | Repository (required when multiple repos configured) |

**Three modes:**

- **Create** (default) — Analyzes issues, reads the codebase, and creates a plan issue with a mermaid dependency graph, actionable issues in order, blocked issues with reasons, and skipped issues. Blocked issues get `forza:needs-human` labels and explanatory comments.

- **Revise** (`--revise <N>`) — Reads the plan issue and its comments, then updates the plan based on human feedback. Adjusts ordering, moves issues between sections, and updates labels.

- **Execute** (`--exec <N>`) — Parses the mermaid dependency graph, topologically sorts it, and processes each actionable issue through its workflow pipeline. Issues with unresolved dependencies are skipped. Completed issues (`forza:complete`) are skipped on re-run, enabling resume after interruption.

### forza status

Show run history and outcomes:

```
forza status [--all] [--detailed] [--run-id <id>] [--workflow <name>]
```

| Flag | Description |
|------|-------------|
| `--all` | Show all runs as a history table (sorted newest first) |
| `--detailed` | Show latest run detail |
| `--run-id <id>` | Show a specific run by ID |
| `--workflow <name>` | Filter dashboard to a single workflow |

Displays recent runs with their route, workflow, outcome, and cost. Also shows plan execution history when plan issues exist.

### forza explain

Visualize your config, routes, and workflows:

```
forza explain [--issues] [--prs] [--conditions] [--route <name>] [--workflows] [--plans] [--json]
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
| `--plans` | Show open plan issues and their execution status |
| `-v` / `--verbose` | Verbose output — show per-stage detail |
| `--json` | Output as JSON |

When no `forza.toml` exists, shows builtin defaults (available workflows, labels, agents).

### forza clean

Remove worktrees and run state:

```
forza clean [--runs] [--stale] [--days <N>] [--dry-run]
```

| Flag | Description |
|------|-------------|
| `--repo-dir <path>` | Repository directory (default: current directory) |
| `--runs` | Remove run state files instead of worktrees |
| `--stale` | Remove only worktrees older than the configured threshold |
| `--days <N>` | Age threshold in days for `--stale` |
| `--dry-run` | Print what would be removed without acting |

### forza open

Open a new GitHub issue using agent assistance:

```
forza open [--repo <owner/name>] [--prompt <text>] [--label <label>] [--ready] [--model <model>]
```

| Flag | Description |
|------|-------------|
| `--repo <owner/name>` | Repository to open an issue in |
| `--prompt <text>` | Prompt describing the issue to open |
| `--label <label>` | Label to apply to the created issue |
| `--ready` | Also add the `forza:ready` label |
| `--model <model>` | Override the model |

### forza serve

Start the REST API server:

```
forza serve [--host <host>] [--port <port>] [--repo-dir <path>]
```

See [REST API](../advanced/api.md) for endpoint documentation.

### forza mcp

Start the MCP server:

```
forza mcp [--http] [--host <host>] [--port <port>]
```

See [MCP Server](../advanced/mcp.md) for tool documentation.

## Global flags

| Flag | Description |
|------|-------------|
| `--config <path>` / `-c` | Path to `forza.toml`. Optional for `issue` and `pr` commands when `--workflow` is provided |
| `--log-file <path>` | Write tracing output to this file instead of stderr |
