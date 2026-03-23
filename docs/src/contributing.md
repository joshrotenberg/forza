# Contributing

## Development setup

```bash
git clone https://github.com/joshrotenberg/forza
cd forza
cargo build --all
```

Requirements:
- Rust 2024 edition (stable toolchain)
- `gh` CLI for integration tests that hit GitHub
- Git configured with user name and email

## Running tests

```bash
# All unit and integration tests
cargo test --all

# Core library tests only
cargo test -p forza-core

# Binary crate unit tests
cargo test -p forza --lib

# Mock-based pipeline integration tests
cargo test -p forza-core --test pipeline_integration
```

## Pre-push checklist

Before pushing:

```bash
cargo fmt --all -- --check
cargo clippy --all --all-targets -- -D warnings
cargo test --all
cargo doc --no-deps --all-features
```

## Project structure

```
Cargo.toml                    (workspace root)
crates/
  forza-core/                 (library — domain model, traits, pipeline)
  forza/                      (binary — CLI, API, MCP, client implementations)
docs/                         mdbook documentation site
examples/                     Example forza.toml configurations
```

## Submitting changes

1. Fork the repository and create a feature branch: `feat/my-change`
2. Follow existing code patterns and Rust 2024 conventions
3. Ensure all tests pass and clippy is clean
4. Open a PR with a clear description and acceptance criteria
5. Label it `forza:ready` if you want forza to review it

## Issue guidelines

See [Writing Issues for Forza](usage/writing-issues.md) — the same guidelines apply whether you are opening an issue for forza to implement or for a human to implement.

## Design principles

Before proposing a new feature, read [What forza isn't](concepts/what-forza-isnt.md) — particularly the feature evaluation guidelines. The key question is: does this belong with the human, the agent, or the framework? Features that blur the responsibility boundary between these three actors are generally out of scope for forza. See the [Architecture](concepts/architecture.md) page for how the codebase is structured.

## License

MIT OR Apache-2.0
