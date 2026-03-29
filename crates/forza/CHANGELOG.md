# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.0](https://github.com/joshrotenberg/forza/compare/forza-v0.3.0...forza-v0.5.0) - 2026-03-29

### Added

- GitHub Action for event-driven forza execution ([#470](https://github.com/joshrotenberg/forza/pull/470))
- *(pipeline)* enrich agent context — comments, route/workflow name, configless plan ([#469](https://github.com/joshrotenberg/forza/pull/469))
- *(api,mcp)* add workflow and model overrides to trigger endpoints ([#465](https://github.com/joshrotenberg/forza/pull/465))
- *(plan)* apply forza:complete/failed label to plan issue after exec closes #451 ([#452](https://github.com/joshrotenberg/forza/pull/452))
- *(explain)* show builtin defaults when no config file exists closes #445 ([#448](https://github.com/joshrotenberg/forza/pull/448))
- *(cli)* auto-dispatch plan issues from forza issue to plan --exec closes #444 ([#447](https://github.com/joshrotenberg/forza/pull/447))
- *(cli)* make forza.toml optional for direct commands ([#443](https://github.com/joshrotenberg/forza/pull/443))
- *(plan)* add --branch flag to target plan branch for PR merges ([#436](https://github.com/joshrotenberg/forza/pull/436))
- *(status)* plan execution tracking in forza status ([#435](https://github.com/joshrotenberg/forza/pull/435))
- *(plan)* concurrent execution of independent issues closes #419 ([#434](https://github.com/joshrotenberg/forza/pull/434))
- *(mcp)* add plan tools to MCP server closes #428 ([#433](https://github.com/joshrotenberg/forza/pull/433))
- *(api)* add plan endpoints for REST API closes #427 ([#432](https://github.com/joshrotenberg/forza/pull/432))
- *(explain)* add --plans flag to show open plan issues and status ([#431](https://github.com/joshrotenberg/forza/pull/431))
- *(plan)* add --close flag to close plan issue after exec closes #421 ([#430](https://github.com/joshrotenberg/forza/pull/430))
- *(cli)* add forza plan command ([#417](https://github.com/joshrotenberg/forza/pull/417))
- *(cli)* include git commit hash in version output ([#412](https://github.com/joshrotenberg/forza/pull/412))
- *(config)* add issue_order to global config for deterministic issue processing ([#399](https://github.com/joshrotenberg/forza/pull/399))

### Fixed

- *(runner)* remove misleading model log from create_agent in configless mode ([#458](https://github.com/joshrotenberg/forza/pull/458))
- *(git)* detect default branch instead of hardcoding origin/main ([#457](https://github.com/joshrotenberg/forza/pull/457))
- *(cli)* add --route flag to forza pr subcommand closes #437 ([#438](https://github.com/joshrotenberg/forza/pull/438))
- *(plan)* deterministic topo sort + dependency merge gating ([#422](https://github.com/joshrotenberg/forza/pull/422))
- *(runner)* deduplicate PRs across condition routes to prevent double-processing ([#410](https://github.com/joshrotenberg/forza/pull/410))
- add homepage, allow-dirty, and homebrew publish job for brew formula ([#397](https://github.com/joshrotenberg/forza/pull/397))

### Other

- *(cli)* remove --route from forza pr, consolidate to --fix/--workflow ([#459](https://github.com/joshrotenberg/forza/pull/459))
- *(cli)* add --workflow flag, fold watch/fix into run/issue/pr ([#440](https://github.com/joshrotenberg/forza/pull/440))

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
