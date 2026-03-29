# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.1](https://github.com/joshrotenberg/forza/compare/forza-core-v0.5.0...forza-core-v0.5.1) - 2026-03-29

### Other

- rationalize builtin workflows ([#481](https://github.com/joshrotenberg/forza/pull/481)) ([#483](https://github.com/joshrotenberg/forza/pull/483))
- release ([#398](https://github.com/joshrotenberg/forza/pull/398))

## [0.5.0](https://github.com/joshrotenberg/forza/compare/forza-core-v0.4.0...forza-core-v0.5.0) - 2026-03-29

### Added

- GitHub Action for event-driven forza execution ([#470](https://github.com/joshrotenberg/forza/pull/470))
- *(pipeline)* enrich agent context — comments, route/workflow name, configless plan ([#469](https://github.com/joshrotenberg/forza/pull/469))
- *(pipeline)* include issue title in draft PR title closes #449 ([#450](https://github.com/joshrotenberg/forza/pull/450))
- *(cli)* add forza plan command ([#417](https://github.com/joshrotenberg/forza/pull/417))

## [0.4.0](https://github.com/joshrotenberg/forza/compare/forza-core-v0.3.0...forza-core-v0.4.0) - 2026-03-24

### Added

- *(tools)* externalize allowed_tools per stage closes #380 ([#381](https://github.com/joshrotenberg/forza/pull/381))

### Other

- release v0.3.0 ([#382](https://github.com/joshrotenberg/forza/pull/382))

## [0.3.0](https://github.com/joshrotenberg/forza/compare/forza-core-v0.1.0...forza-core-v0.3.0) - 2026-03-23

### Added

- *(init)* ship built-in language context templates closes #375 ([#378](https://github.com/joshrotenberg/forza/pull/378))

### Fixed

- *(prompts)* remove Rust-specific language from core prompt templates ([#377](https://github.com/joshrotenberg/forza/pull/377))
- bump forza-core version to 0.3.0 to resolve release tag conflict ([#373](https://github.com/joshrotenberg/forza/pull/373))

## [0.1.0](https://github.com/joshrotenberg/forza/releases/tag/forza-core-v0.1.0) - 2026-03-23

### Added

- *(pipeline)* post failure reason as comment on issue closes #335 ([#338](https://github.com/joshrotenberg/forza/pull/338))
- *(cli)* add open subcommand for agent-driven issue creation closes #327 ([#337](https://github.com/joshrotenberg/forza/pull/337))
- *(github)* add create_issue to GitHubClient trait and all implementations closes #325 ([#329](https://github.com/joshrotenberg/forza/pull/329))
- *(planner)* add open_issue.md prompt template ([#328](https://github.com/joshrotenberg/forza/pull/328))
- *(planner)* agent-specific prompt directory overrides ([#320](https://github.com/joshrotenberg/forza/pull/320))
- *(logging)* log agent backend and model at info level for every run ([#313](https://github.com/joshrotenberg/forza/pull/313))
- add mock-based integration test framework ([#285](https://github.com/joshrotenberg/forza/pull/285))
- add DraftPr stage for early draft PR visibility ([#271](https://github.com/joshrotenberg/forza/pull/271))

### Fixed

- *(pipeline)* truncate failure comment from beginning, not end closes #345 ([#346](https://github.com/joshrotenberg/forza/pull/346))
- add [skip ci] to draft PR empty commit ([#339](https://github.com/joshrotenberg/forza/pull/339))
- create empty commit in draft_pr.sh so branch has diff from main ([#298](https://github.com/joshrotenberg/forza/pull/298))
- chain merge.sh commands with && so CI failure prevents merge ([#297](https://github.com/joshrotenberg/forza/pull/297))
- *(forza-core)* add Display impl for Scope enum closes #290 ([#291](https://github.com/joshrotenberg/forza/pull/291))
- *(draft-pr)* stop committing breadcrumb files to branch closes #282 ([#284](https://github.com/joshrotenberg/forza/pull/284))
- *(adapters)* pass actual StageKind through AgentExecutor::execute closes #257 ([#281](https://github.com/joshrotenberg/forza/pull/281))
- *(merge)* remove --delete-branch flag to avoid worktree conflict closes #277 ([#279](https://github.com/joshrotenberg/forza/pull/279))
- *(stage)* wait for CI checks before merging closes #275 ([#276](https://github.com/joshrotenberg/forza/pull/276))
- split git add in draft_pr.sh to handle missing .forza/ directory ([#274](https://github.com/joshrotenberg/forza/pull/274))
- *(forza-core)* add Display impl for Execution enum closes #272 ([#273](https://github.com/joshrotenberg/forza/pull/273))
- *(pipeline)* detect pr after open_pr stage to set correct run outcome closes #260 ([#262](https://github.com/joshrotenberg/forza/pull/262))
- *(forza-core)* add Display impl for StageStatus closes #253 ([#259](https://github.com/joshrotenberg/forza/pull/259))

### Other

- add integration test for condition route discovery ([#308](https://github.com/joshrotenberg/forza/pull/308))
- add [workspace.dependencies] to deduplicate shared dep versions ([#267](https://github.com/joshrotenberg/forza/pull/267))
- add [workspace.package] to deduplicate crate metadata ([#266](https://github.com/joshrotenberg/forza/pull/266))
- *(forza-core)* add module-level export table to lib.rs docs ([#261](https://github.com/joshrotenberg/forza/pull/261))
- forza-core crate and unified pipeline ([#252](https://github.com/joshrotenberg/forza/pull/252))
