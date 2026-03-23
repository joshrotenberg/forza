# Getting Started

## Installation

```bash
cargo install forza
```

## Initialize a repository

Run `forza init` to create the required GitHub labels and generate a starter `forza.toml`:

```bash
forza init --repo owner/name
```

This creates the `forza:ready`, `forza:in-progress`, `forza:complete`, `forza:failed`, and `forza:needs-human` labels in your repository and writes a minimal `forza.toml` to the current directory.

## Minimal configuration

The simplest possible setup — one repo, one bug route, all defaults:

```toml
[global]
model = "claude-sonnet-4-6"

[security]
authorization_level = "contributor"

[repos."your-org/your-repo"]

[repos."your-org/your-repo".routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"
```

Save this as `forza.toml` in your working directory.

## Process your first issue

Label an issue in your repository with `bug` (and optionally `forza:ready` if you set `gate_label`), then run:

```bash
# Process a single issue by number
forza issue 123

# Preview without executing
forza issue 123 --dry-run
```

## Common commands

```bash
# Process a single issue
forza issue 123

# Fix a PR (rebase + fix CI)
forza pr 42

# Run one batch cycle (discover + process all eligible issues)
forza run

# Continuous polling loop
forza watch --interval 60

# View run history
forza status

# Visualize your config, routes, and workflows
forza explain
```

## Next steps

- Read [Concepts](concepts/overview.md) to understand routes, workflows, and stages
- See [Configuration Reference](configuration/reference.md) for all available options
- Browse [Examples](configuration/examples.md) for common configurations
- Learn how to [write effective issues](usage/writing-issues.md) for best results
