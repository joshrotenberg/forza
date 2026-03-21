# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0](https://github.com/joshrotenberg/forza/compare/v0.2.0...v0.3.0) - 2026-03-21

### Added

- route conditions, retry budget, and PR health monitoring ([#104](https://github.com/joshrotenberg/forza/pull/104))

### Other

- add Cargo.toml keywords and categories for crates.io discoverability closes #86 ([#106](https://github.com/joshrotenberg/forza/pull/106))

## [0.2.0](https://github.com/joshrotenberg/forza/compare/v0.1.0...v0.2.0) - 2026-03-21

### Added

- *(mcp)* embed MCP server with runner, status, and config tool groups closes #52 ([#101](https://github.com/joshrotenberg/forza/pull/101))
- REST API ([#100](https://github.com/joshrotenberg/forza/pull/100))
- security hardening — prompt injection, authz, rate limiting ([#72](https://github.com/joshrotenberg/forza/pull/72))
- pr-fix workflow and route config updates ([#97](https://github.com/joshrotenberg/forza/pull/97))
- reactive workflow mode for PR maintenance ([#92](https://github.com/joshrotenberg/forza/pull/92))
- configurable stage prompt templates ([#61](https://github.com/joshrotenberg/forza/pull/61))
- *(isolation)* auto-detect repo dir and clone if needed closes #69 ([#90](https://github.com/joshrotenberg/forza/pull/90))
- *(deps)* validate gh, git, and agent CLI on startup closes #70 ([#89](https://github.com/joshrotenberg/forza/pull/89))
- PR workflows — label-driven PR processing ([#88](https://github.com/joshrotenberg/forza/pull/88))
- skill injection and MCP server access for stages ([#87](https://github.com/joshrotenberg/forza/pull/87))
- *(notifications)* desktop, slack, and webhook alerts on run completion closes #17 ([#73](https://github.com/joshrotenberg/forza/pull/73))
- *(config)* add multi-repo support with per-repo routes closes #19 ([#71](https://github.com/joshrotenberg/forza/pull/71))
- *(orchestrator)* rich PR descriptions from run data and breadcrumbs closes #64 ([#67](https://github.com/joshrotenberg/forza/pull/67))
- schedule windows for routes ([#66](https://github.com/joshrotenberg/forza/pull/66))
- *(orchestrator)* initialize active counts from in-progress labels and add concurrency tracing closes #21 ([#65](https://github.com/joshrotenberg/forza/pull/65))
- *(github)* add automation-optimized issue templates closes #22 ([#63](https://github.com/joshrotenberg/forza/pull/63))
- *(config)* add tests for custom workflow template resolution closes #23 ([#62](https://github.com/joshrotenberg/forza/pull/62))
- *(workflow)* add auto-merge stage gated by global.auto_merge closes #55 ([#58](https://github.com/joshrotenberg/forza/pull/58))
- *(orchestrator)* concurrent batch processing via JoinSet closes #56 ([#57](https://github.com/joshrotenberg/forza/pull/57))
- tool call tracing in executor via streaming ([#54](https://github.com/joshrotenberg/forza/pull/54))
- per-route agent config (model, skills, MCP) ([#51](https://github.com/joshrotenberg/forza/pull/51))
- per-stage hooks (pre/post/finally) ([#48](https://github.com/joshrotenberg/forza/pull/48))
- *(dry-run)* show estimated cost from run history closes #16 ([#47](https://github.com/joshrotenberg/forza/pull/47))
- *(init)* add forza init command — create labels and starter config closes #35 ([#46](https://github.com/joshrotenberg/forza/pull/46))
- fix command — re-run failed stages ([#13](https://github.com/joshrotenberg/forza/pull/13)) ([#45](https://github.com/joshrotenberg/forza/pull/45))
- agentless stages and issue comments in prompts (#26, #28) ([#44](https://github.com/joshrotenberg/forza/pull/44))

### Other

- add badges to README ([#99](https://github.com/joshrotenberg/forza/pull/99))
- forza tag line ([#98](https://github.com/joshrotenberg/forza/pull/98))
- *(config)* add research and chore routes to forza.toml closes #68 ([#91](https://github.com/joshrotenberg/forza/pull/91))
- cargo dist config with homebrew, shell, powershell installers ([#43](https://github.com/joshrotenberg/forza/pull/43))
- cargo dist setup for releases ([#41](https://github.com/joshrotenberg/forza/pull/41))
- release v0.1.0 ([#40](https://github.com/joshrotenberg/forza/pull/40))

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
