# Getting Started

## Quickstart

### 1. Install forza

**Homebrew (macOS and Linux):**

```bash
brew install joshrotenberg/brew/forza
```

**Shell installer (macOS and Linux):**

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/joshrotenberg/forza/releases/latest/download/forza-installer.sh | sh
```

**PowerShell installer (Windows):**

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://github.com/joshrotenberg/forza/releases/latest/download/forza-installer.ps1 | iex"
```

**Cargo:**

```bash
cargo install forza
```

### 2. Initialize your repository

Run the guided setup in your repository directory:

```bash
forza init --repo owner/name
```

This creates the required GitHub labels (`forza:ready`, `forza:in-progress`, `forza:complete`, `forza:failed`, `forza:needs-human`) and writes a starter `forza.toml` to the current directory.

### 3. Process your first issue

Label an issue with `bug` and `forza:ready`, then run:

```bash
forza issue 123
```

Forza picks up the issue, plans the fix, implements it, runs tests, and opens a PR.

To preview without executing:

```bash
forza issue 123 --dry-run
```

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
