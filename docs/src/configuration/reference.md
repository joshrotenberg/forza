# Configuration Reference

Complete field reference for `forza.toml`.

## [global]

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `model` | string | — | Default agent model (e.g., `claude-sonnet-4-6`) |
| `agent` | string | `"claude"` | Agent backend: `"claude"` or `"codex"` |
| `gate_label` | string | — | Label required on issues/PRs before processing; omit to process all matching subjects |
| `branch_pattern` | string | `"automation/{issue}-{slug}"` | Branch name pattern; `{issue}` = number, `{slug}` = slugified title |
| `max_concurrency` | integer | — | Total parallel runs across all repos and routes |
| `max_cost_per_issue` | float | — | Stop a run if it exceeds this USD cost |
| `max_cost_per_hour` | float | — | Pause all routes if hourly spend exceeds this USD amount |
| `auto_merge` | bool | `false` | Automatically merge PRs once CI is green (deprecated; prefer a `merge` stage) |

## [security]

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `authorization_level` | string | `"contributor"` | `sandbox` \| `local` \| `contributor` \| `trusted` — controls agent permissions |
| `allowed_authors` | string[] | `[]` | Only process issues from these GitHub usernames; empty = authenticated user only |
| `require_label` | string | — | Additional label that must be present before processing; never removed automatically |

### Authorization levels

| Level | Agent can |
|-------|-----------|
| `sandbox` | Read files only; no writes, no shell commands |
| `local` | Read and write files; no shell execution |
| `contributor` | Read, write, and run pre-approved shell commands |
| `trusted` | Full access; unrestricted shell execution |

## [validation]

| Field | Type | Description |
|-------|------|-------------|
| `commands` | string[] | Shell commands run between every stage; all must exit 0 |

## [agent_config]

| Field | Type | Description |
|-------|------|-------------|
| `skills` | string[] | Paths to skill files prepended to every agent prompt |
| `mcp_config` | string | Path to an MCP server config file |
| `append_system_prompt` | string | Text appended to the system prompt for every agent invocation |

## [stage_hooks.\<kind\>]

Replace `<kind>` with a stage kind name (e.g., `implement`, `test`, `open_pr`).

| Field | Type | Description |
|-------|------|-------------|
| `pre` | string[] | Commands run before the stage |
| `post` | string[] | Commands run after a successful stage |
| `finally` | string[] | Commands run after the stage regardless of success or failure |

## [repos."owner/name"]

| Field | Type | Description |
|-------|------|-------------|
| `repo_dir` | string | Path to the local git checkout; defaults to current directory |

## [repos."owner/name".routes.\<name\>]

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | yes | `"issue"` or `"pr"` |
| `label` | string | label routes | GitHub label that triggers this route |
| `condition` | string | condition routes | PR condition that triggers this route |
| `workflow` | string | yes | Workflow template name |
| `scope` | string | no | `"forza_owned"` (default) or `"all"` — which PRs to evaluate |
| `concurrency` | integer | no | Max parallel runs for this route |
| `poll_interval` | integer | no | Seconds between discovery polls |
| `max_retries` | integer | no | Failures before applying `forza:needs-human` |
| `model` | string | no | Model override for this route |
| `skills` | string[] | no | Additional skill files for this route |

## [[workflow_templates]]

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | yes | Template name; overrides built-in if names match |
| `stages` | Stage[] | yes | Ordered list of stages |

### Stage fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `kind` | string | yes | Stage kind (see [Stage Kinds](../concepts/stage-kinds.md)) |
| `optional` | bool | no | Skip rather than fail if not applicable |
| `condition` | string | no | Shell command; skip stage if exit code is non-zero |
| `max_retries` | integer | no | Stage-level retry limit |
| `command` | string | no | Agentless: shell command to run instead of invoking the agent |
