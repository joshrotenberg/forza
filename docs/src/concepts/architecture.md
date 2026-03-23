# Architecture

Forza uses a small set of core abstractions and a unified execution pipeline. Issues and PRs follow the same code path — differences are data, not control flow.

## Core abstractions

### Subject

The thing being worked on. Unifies issues and PRs into a single type.

A subject carries the GitHub metadata needed for routing and prompt generation: number, title, body, labels, and (for PRs) branch, mergeable state, check status, and review decision.

### MatchedWork

A subject bound to its route. Created once at discovery time and never re-matched. This struct flows through the entire pipeline unchanged, ensuring the human's intent (expressed as a label or PR state) is locked in.

### Route

A trigger-to-workflow mapping. Each route specifies a subject type (issue or PR), a trigger (label or condition), a workflow name, and optional overrides for model, concurrency, and retry limits. See [Routes](routes.md) for configuration details.

### Workflow

An ordered list of stages. Always linear — no branching or reactive dispatch. See [Workflows](workflows.md) for built-in templates and custom definitions.

### Run

A single execution of a workflow against a subject. Tracks stage records, timing, status, and outcome. Run records are persisted to disk for `forza status` and retry tracking.

## The pipeline

The entire execution model flows through a single function:

```
discover -> match -> execute -> report
```

### Discovery

Fetches eligible subjects from GitHub. Three sources:

1. **Gate-labeled issues** — issues with `forza:ready` (or configured gate label)
2. **Label-routed PRs** — PRs with a route-specific label
3. **Condition-routed PRs** — all open PRs evaluated against condition routes

Discovery produces a list of subject-route pairs. The route is determined here and never re-evaluated.

### Matching

For issues, matching is label-based — find the route whose label matches one of the issue's labels. For condition-routed PRs, matching already happened during discovery. The output is `MatchedWork`: subject + route + resolved workflow.

### Execution

One function handles both issues and PRs:

1. **Acquire lease** — add the in-progress label
2. **Create worktree** — if the workflow needs one (some like `pr-merge` do not)
3. **Build stage prompts** — the planner generates prompts from the subject and workflow
4. **Execute stages** — for each stage:
   - Evaluate the stage condition (if any)
   - Run pre-hooks
   - Execute (agent invocation or shell command)
   - Run post-hooks and finally-hooks
   - Load breadcrumb for the next stage
   - Run validation commands between stages
   - Stop on non-optional failure
5. **Release lease** — remove in-progress label, add complete or failed label
6. **Cleanup worktree**
7. **Persist run record**
8. **Notify**

### Issue vs PR: where the differences live

| Concern | Issue | PR |
|---------|-------|-----|
| Prompt generation | Uses issue title/body, `{issue_number}` | Uses PR title/body, `{pr_number}`, `{head_branch}` |
| Open PR stage | Creates a new PR | Updates existing PR body |
| Merge stage | Merges the created PR | Merges the existing PR |
| Label API | Issue label endpoints | PR label endpoints |
| Branch | Generated from pattern | Already exists (PR head) |
| Env vars | `FORZA_ISSUE_NUMBER` | `FORZA_PR_NUMBER` |

These differences are handled by checking `subject.kind` in the planner, lifecycle module, and shell environment setup. No separate code paths.

## Scheduling

The batch/poll loop is a scheduler that:

1. Discovers eligible work each cycle
2. Filters by concurrency limits (global `max_concurrency` and per-route `concurrency`)
3. Spawns work that fits within limits
4. Defers remaining work to the next cycle
5. Collects completed runs

## Crate structure

```
Cargo.toml                        (workspace root)
crates/
  forza-core/                     (library -- domain model, traits, pipeline)
    src/
      condition.rs                RouteCondition evaluation
      error.rs                    Error types
      lifecycle.rs                Label management (in-progress, complete, failed)
      pipeline.rs                 Unified stage execution
      planner.rs                  Prompt generation from Subject + Workflow
      route.rs                    Route, Trigger, Scope, MatchedWork
      run.rs                      Run, RunStatus, Outcome, StageRecord
      shell.rs                    Shell execution with FORZA_* env vars
      stage.rs                    StageKind, Stage, Execution, Workflow, builtins
      subject.rs                  Subject (unified issue/PR type), SubjectKind
      traits.rs                   GitHubClient, GitClient, AgentExecutor traits
  forza/                          (binary -- CLI, API, MCP, client implementations)
    src/
      adapters.rs                 Agent and client adapters
      runner.rs                   Discovery, scheduling, pipeline execution
      config.rs                   RunnerConfig, Route, GlobalConfig (TOML parsing)
      main.rs                     CLI (clap)
      api.rs                      REST API (axum)
      mcp.rs                      MCP server (tower-mcp)
      github/                     GitHubClient implementations (gh CLI, octocrab)
      git/                        GitClient implementations (git CLI, gix)
```

## Environment variables

All shell commands (agentless stages, conditions, hooks, validation) receive these environment variables:

| Variable | Always set | Value |
|----------|-----------|-------|
| `FORZA_REPO` | yes | `owner/name` |
| `FORZA_SUBJECT_TYPE` | yes | `issue` or `pr` |
| `FORZA_SUBJECT_NUMBER` | yes | issue/PR number |
| `FORZA_ISSUE_NUMBER` | issues only | issue number |
| `FORZA_PR_NUMBER` | PRs only | PR number |
| `FORZA_BRANCH` | yes | branch name |
| `FORZA_RUN_ID` | yes | run ID |
| `FORZA_ROUTE` | yes | route name |
| `FORZA_WORKFLOW` | yes | workflow name |
