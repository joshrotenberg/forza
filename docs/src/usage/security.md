# Security

## Shell command trust boundary

All shell commands that forza executes — validation commands, stage hooks, agentless stage commands, and stage conditions — are run via `sh -c` with the string taken directly from your `forza.toml`. This is by design: the config file is the trust boundary.

**What this means in practice:**

- Any string in `[validation].commands`, `[stage_hooks.*]` hook lists, agentless stage `command` fields, and stage `condition` fields has full shell access to your environment with the privileges of the forza process.
- There is no sandboxing or command allowlisting for these config-driven shell invocations.

**The assumption:** `forza.toml` is trusted. Treat it like executable code — review changes to it the same way you would review code changes.

**The risk:** If forza ever processes config from an untrusted source (for example, a PR that modifies `forza.toml`), those commands become an arbitrary code execution vector. Forza does not currently validate or restrict the content of commands in config.

**Mitigations:**

- Store `forza.toml` in version control and require review for all changes to it.
- Do not configure forza to automatically apply config changes submitted by untrusted contributors without explicit review.
- Run forza with the minimum privileges needed for your workflows.
- Note: `authorization_level` in `[security]` controls what the Claude agent can do, not what forza's own shell invocations can do. It does not restrict hooks, validation commands, agentless stages, or conditions.

## Authorization levels

The `authorization_level` field in `[security]` controls what the agent is permitted to do during a run:

| Level | Agent can |
|-------|-----------|
| `sandbox` | Read files only; no writes, no shell commands |
| `local` | Read and write files in the worktree; no shell execution |
| `contributor` | Read, write, and run pre-approved shell commands |
| `trusted` | Full access; unrestricted shell execution |

For production or public-facing repositories, prefer `contributor` or `sandbox`. For internal repos where you control who applies the gate label, `trusted` is appropriate.

## Allowed authors

Use `allowed_authors` to restrict which GitHub users can trigger runs:

```toml
[security]
authorization_level = "contributor"
allowed_authors = ["your-github-username", "trusted-collaborator"]
```

An empty list (the default) restricts processing to the authenticated user only.

## Require label

The `require_label` field adds a secondary approval gate that must be present on the issue before processing begins:

```toml
[security]
authorization_level = "contributor"
require_label = "security:approved"
```

Unlike `gate_label` (which is removed when forza picks up an issue), `require_label` is a permanent gate that is never removed automatically. A human must apply it to explicitly approve processing.

## GitHub credentials

Forza uses the `gh` CLI for GitHub operations. Ensure `gh` is authenticated with appropriate permissions:

```bash
gh auth login
gh auth status
```

The forza process inherits whatever permissions your `gh` session has. Scope it to the minimum required: typically `repo` access for the repositories you are automating.
