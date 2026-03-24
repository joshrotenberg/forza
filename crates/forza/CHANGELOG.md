# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.1](https://github.com/joshrotenberg/forza/compare/forza-v0.3.0...forza-v0.3.1) - 2026-03-24

### Fixed

- add homepage, allow-dirty, and homebrew publish job for brew formula ([#397](https://github.com/joshrotenberg/forza/pull/397))

## [0.3.0](https://github.com/joshrotenberg/forza/compare/forza-v0.2.0...forza-v0.3.0) - 2026-03-23

### Added

- *(cli)* add open subcommand for agent-driven issue creation closes #327 ([#337](https://github.com/joshrotenberg/forza/pull/337))
- *(runner)* multi-prefix forza_owned scope matching ([#334](https://github.com/joshrotenberg/forza/pull/334))
- *(github)* add create_issue to GitHubClient trait and all implementations closes #325 ([#329](https://github.com/joshrotenberg/forza/pull/329))
- *(runner)* add {route} and {label} placeholders to branch_pattern ([#324](https://github.com/joshrotenberg/forza/pull/324))
- *(planner)* agent-specific prompt directory overrides ([#320](https://github.com/joshrotenberg/forza/pull/320))
- *(config)* add per-route branch_pattern override ([#319](https://github.com/joshrotenberg/forza/pull/319))
- *(logging)* log agent backend and model at info level for every run ([#313](https://github.com/joshrotenberg/forza/pull/313))
- *(run)* add --route filter to forza run command ([#307](https://github.com/joshrotenberg/forza/pull/307))
- add Codex agent backend support ([#293](https://github.com/joshrotenberg/forza/pull/293))
- enhance forza explain with filters, grouping, and verbose mode ([#286](https://github.com/joshrotenberg/forza/pull/286))
- add DraftPr stage for early draft PR visibility ([#271](https://github.com/joshrotenberg/forza/pull/271))

### Fixed

- replace absolute symlink in test-helpers with relative symlink ([#366](https://github.com/joshrotenberg/forza/pull/366))
- *(runner)* change empty code fence to text to silence rustdoc warning ([#354](https://github.com/joshrotenberg/forza/pull/354))
- *(executor)* read skill files and prepend to prompt instead of using --file ([#331](https://github.com/joshrotenberg/forza/pull/331))
- *(cli)* update --repo-dir doc comment on RunArgs to match issue/pr commands ([#309](https://github.com/joshrotenberg/forza/pull/309))
- *(explain)* show agent backend in global header and verbose route output closes #302 ([#305](https://github.com/joshrotenberg/forza/pull/305))
- update init guided prompt with correct workflow names and next steps ([#296](https://github.com/joshrotenberg/forza/pull/296))
- *(adapters)* pass actual StageKind through AgentExecutor::execute closes #257 ([#281](https://github.com/joshrotenberg/forza/pull/281))
- suppress noisy HTTP/TLS debug logs with smarter default filter ([#280](https://github.com/joshrotenberg/forza/pull/280))
- *(github)* improve error messages for failed API calls and missing issues closes #265 ([#270](https://github.com/joshrotenberg/forza/pull/270))

### Other

- add [workspace.dependencies] to deduplicate shared dep versions ([#267](https://github.com/joshrotenberg/forza/pull/267))
- *(runner)* add doc comment to generate_branch ([#268](https://github.com/joshrotenberg/forza/pull/268))
- add [workspace.package] to deduplicate crate metadata ([#266](https://github.com/joshrotenberg/forza/pull/266))
- forza-core crate and unified pipeline ([#252](https://github.com/joshrotenberg/forza/pull/252))
