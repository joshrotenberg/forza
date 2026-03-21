# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- *(dry-run)* show estimated cost range from historical run data in `--dry-run` output closes #16

## [0.1.0](https://github.com/joshrotenberg/forza/releases/tag/v0.1.0) - 2026-03-21

### Added

- *(orchestrator)* breadcrumbs between stages for context flow closes #7 ([#36](https://github.com/joshrotenberg/forza/pull/36))
- *(clean)* add clean command to remove worktrees and run state closes #14 ([#31](https://github.com/joshrotenberg/forza/pull/31))
- *(status)* add --all and --summary flags for run history dashboard closes #15
- initial forza — autonomous GitHub issue runner

### Fixed

- *(orchestrator)* add signal handling and stale lease recovery closes #5 ([#37](https://github.com/joshrotenberg/forza/pull/37))
- use absolute path for claude-wrapper dep (worktree compat)

### Other

- full CI suite (fmt, clippy, test, msrv, docs, release-plz) ([#39](https://github.com/joshrotenberg/forza/pull/39))
- stabilize for v0.1.0 ([#38](https://github.com/joshrotenberg/forza/pull/38))
- decouple stage prompts from language-specific commands ([#33](https://github.com/joshrotenberg/forza/pull/33))
- CI workflow, crates.io dep, README rename ([#32](https://github.com/joshrotenberg/forza/pull/32))
- sync main with master, keep absolute path dep
- add CLAUDE.md, forza.toml, .gitignore, migrate issues
- first
