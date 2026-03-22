# Contributing to forza

Thank you for your interest in contributing to forza.

## Prerequisites

- Rust (MSRV: 1.90.0)
- `gh` CLI (authenticated)
- `git`

## Development setup

```bash
git clone https://github.com/joshrotenberg/forza
cd forza
cargo build
```

## Before submitting a PR

Run the full pre-commit checklist:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --lib --all-features
cargo test --test '*' --all-features
cargo doc --no-deps --all-features
```

All checks must pass. The CI pipeline runs these automatically.

## Submitting changes

1. Fork the repository and create a branch:
   - `fix/` for bug fixes
   - `feat/` for new features
   - `docs/` for documentation
   - `refactor/` for refactoring
   - `test/` for test improvements

2. Make your changes following the patterns in the existing code.

3. Write or update tests. All tests live in `#[cfg(test)]` modules inside the source files — there are no separate integration test files.

4. Open a pull request against `main`. Reference any related issues in the PR description.

## Commit style

Use [conventional commits](https://www.conventionalcommits.org/):

```
feat: add schedule window support
fix(orchestrator): handle stale lease on startup
docs: update CLI reference in README
```

Breaking changes use `feat!:` or `fix!:`.

## Code conventions

- Rust 2024 edition — use if-let chains (`if let Some(x) = y && condition {`) instead of nested blocks
- `thiserror` for library errors, `anyhow` for application errors
- All public APIs must have doc comments
- No emojis in code, commits, or documentation

## Reporting issues

Use the GitHub issue templates:
- **Bug report** — unexpected behavior with steps to reproduce
- **Feature request** — describe the use case and desired outcome

## License

By contributing, you agree that your contributions will be licensed under the same terms as the project: MIT OR Apache-2.0.
