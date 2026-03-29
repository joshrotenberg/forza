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

### 2. Process your first issue

No configuration needed. From inside any repo with a GitHub remote:

```bash
forza issue 42 --workflow feature
```

Forza picks up the issue, plans the fix, implements it, runs tests, and opens a PR.

To preview without executing:

```bash
forza issue 42 --workflow feature --dry-run
```

### 3. Add configuration (optional)

For automated discovery and continuous processing, initialize your repo:

```bash
forza init --repo owner/name
```

This creates the required GitHub labels and writes a starter `forza.toml`. Then:

```bash
# Run one batch cycle
forza run

# Continuous polling
forza run --watch
```

## Minimal configuration

The simplest setup — one repo, one bug route:

```toml
[global]
model = "claude-sonnet-4-6"

[repos."your-org/your-repo".routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"
```

Save as `forza.toml` in your working directory.

## Common commands

```bash
# Process a single issue (configless)
forza issue 42 --workflow feature

# Process a single issue (with config, route matching)
forza issue 42

# Fix a PR
forza pr 123 --workflow pr-fix

# Re-run a failed issue with error context
forza issue 42 --fix

# Plan a batch of issues
forza plan 10 20 30

# Execute a plan in dependency order
forza plan --exec 99

# Run one batch cycle (requires config)
forza run

# Continuous polling (requires config)
forza run --watch --interval 60

# View run history
forza status

# Visualize config, routes, and workflows
forza explain
```

## Next steps

- Read [Concepts](concepts/overview.md) to understand routes, workflows, and stages
- See [Configuration Reference](configuration/reference.md) for all available options
- Browse [Examples](configuration/examples.md) for common configurations
- Learn how to [write effective issues](usage/writing-issues.md) for best results
