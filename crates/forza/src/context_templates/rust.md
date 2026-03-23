# Rust project context

## Language and toolchain

This is a Rust project. Use `cargo` for all build, test, and lint operations.

## Common commands

```bash
cargo build                          # compile
cargo test                           # run all tests
cargo test --lib                     # unit tests only
cargo test --test '*'                # integration tests only
cargo clippy --all-targets -- -D warnings   # lint (treat warnings as errors)
cargo fmt --all                      # format code
cargo fmt --all -- --check           # check formatting without modifying
cargo doc --no-deps                  # build documentation
```

## Conventions

- Use `thiserror` for library error types, `anyhow` for application error handling.
- Public APIs must have doc comments (`///`).
- Prefer `#[derive(Debug, Clone)]` on structs and enums where appropriate.
- Use `#[serde(default)]` for optional config fields.
- Write unit tests in `#[cfg(test)]` blocks in the same file.
- Write integration tests under `tests/`.
- Use `tempfile::tempdir()` for filesystem tests.

## Workspace layout

Check `Cargo.toml` for workspace members and dependency versions.
