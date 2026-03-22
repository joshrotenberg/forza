# forza v2 — Design Document

## Motivation

forza works. The concept is validated: deterministic multi-stage workflows with agents
spliced in at the right moments. But the implementation has outgrown its structure.
Recent bugs (auto-merge ping-pong, route re-matching, missing env vars in linear vs
reactive paths) aren't technically hard — they're hard because of accidental complexity.

Three processing paths (`process_issue_with_overrides`, `process_pr_with_overrides`,
`process_reactive_pr`) do similar things with subtle differences. Global config flags
(`auto_merge`, `draft_pr`) secretly modify stage behavior. Routes are matched twice
(once at discovery, once at processing). The orchestrator module is a god object that
handles discovery, matching, concurrency, worktrees, execution, labels, breadcrumbs,
notifications, and state persistence.

This document proposes a refactor that preserves all features while collapsing the
complexity into a single execution path.

## Design Principles

1. **One route, one action, one cycle.** A route does exactly one thing. If a PR needs
   rebasing and CI fixing and merging, that's three routes across three poll cycles.

2. **GitHub is the state machine.** PR state on GitHub is authoritative. Routes are
   transition functions. The poll loop is the event loop. No internal state machine.

3. **Match once, carry through.** Once a subject is matched to a route, that binding is
   immutable for the duration of the run. No re-matching.

4. **Config is behavior.** Everything a route does should be visible in the TOML. No
   global flags that secretly modify stages.

5. **One path through the code.** Issues and PRs follow the same execution pipeline.
   Differences are data, not control flow.

## Core Abstractions

### Subject

The thing being worked on. Unifies issues and PRs.

```rust
enum SubjectKind { Issue, Pr }

struct Subject {
    kind: SubjectKind,
    number: u64,
    repo: String,
    title: String,
    body: String,
    labels: Vec<String>,
    branch: Option<String>,   // PR head branch, or generated for issues
    // PR-specific (None for issues)
    mergeable: Option<String>,
    checks_passing: Option<bool>,
    review_decision: Option<String>,
}
```

### MatchedWork

A subject bound to its route. Created once, never re-matched.

```rust
struct MatchedWork {
    subject: Subject,
    route_name: String,
    route: Route,
    workflow: Workflow,
}
```

### Route

A trigger-to-workflow mapping. Unchanged from current, but no reactive mode.

```rust
struct Route {
    name: String,
    subject_type: SubjectType,    // Issue or Pr
    trigger: Trigger,             // Label(String) or Condition(RouteCondition)
    workflow: String,
    scope: Scope,                 // All or ForzaOwned
    concurrency: usize,
    poll_interval: u64,
    max_retries: Option<usize>,
    // per-route overrides
    model: Option<String>,
    skills: Option<Vec<String>>,
    mcp_config: Option<String>,
    validation_commands: Option<Vec<String>>,
}
```

### Workflow

An ordered list of stages. Always linear. No reactive mode.

```rust
struct Workflow {
    name: String,
    stages: Vec<Stage>,
}

struct Stage {
    kind: StageKind,
    execution: Execution,
    optional: bool,
    condition: Option<String>,     // shell command, exit 0 = run
}

enum Execution {
    Agent,                         // invoke Claude with a generated prompt
    Shell { command: String },     // run a shell command directly
}
```

### Run

A single execution of a workflow against a subject.

```rust
struct Run {
    id: String,
    subject_number: u64,
    repo: String,
    route: String,
    workflow: String,
    branch: String,
    stages: Vec<StageRecord>,
    status: RunStatus,
    outcome: Outcome,
    started_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
}
```

## The Pipeline

The entire execution model is a single function:

```
discover → match → execute → report
```

### Discovery

Fetch eligible subjects from GitHub. Three sources:

1. **Gate-labeled issues**: issues with `forza:ready` (or configured gate label)
2. **Label-routed PRs**: PRs with a route-specific label (`forza:fix-pr`, etc.)
3. **Condition-routed PRs**: all open PRs evaluated against condition routes

Discovery produces a list of `(Subject, route_name)` pairs. The route is determined
here and never re-evaluated.

```rust
pub async fn discover(
    repo: &str,
    config: &Config,
    routes: &IndexMap<String, Route>,
    gh: &dyn GitHubClient,
) -> Vec<(Subject, String)> { ... }
```

### Matching

For issues, matching is label-based (find the route whose label matches one of the
issue's labels). For condition-routed PRs, matching already happened during discovery.
For label-routed PRs, matching is the same as issues.

The output is `MatchedWork` — subject + route + resolved workflow. This struct flows
through the entire pipeline unchanged.

### Execution

One function. One path. Issues and PRs alike.

```rust
pub async fn execute(
    work: &MatchedWork,
    config: &Config,
    state_dir: &Path,
    repo_dir: &Path,
    gh: &dyn GitHubClient,
    git: &dyn GitClient,
    agent: &dyn AgentExecutor,
) -> Run {
    let mut run = Run::new(work);

    // 1. Acquire lease (add in-progress label)
    lifecycle::acquire(&work.subject, config, gh).await;

    // 2. Create worktree (if workflow needs one)
    let worktree = if work.workflow.needs_worktree {
        Some(isolation::create(repo_dir, &work.subject.branch, git).await?)
    } else {
        None
    };
    let work_dir = worktree.as_deref().unwrap_or(repo_dir);

    // 3. Build stage prompts
    let plan = planner::create(work, config, &run.id);

    // 4. Execute stages
    for (i, planned) in plan.stages.iter().enumerate() {
        // Evaluate condition
        if let Some(cond) = &planned.condition {
            if !shell::check(cond, work_dir, &work.subject).await {
                if planned.optional {
                    run.record_skipped(planned);
                    continue;
                }
                run.record_failed(planned, "condition not met");
                break;
            }
        }

        // Pre-hooks
        if let Err(e) = hooks::run_pre(&planned.kind, config, work_dir).await {
            run.record_failed(planned, &e.to_string());
            break;
        }

        // Execute
        let result = match &planned.execution {
            Execution::Agent => {
                agent.execute(planned, work_dir, config, &work.route).await
            }
            Execution::Shell { command } => {
                shell::execute(command, work_dir, &work.subject).await
            }
        };

        // Post-hooks
        hooks::run_post(&planned.kind, config, work_dir, &result).await;

        // Run-hooks (finally)
        hooks::run_finally(&planned.kind, config, work_dir).await;

        // Load breadcrumb for next stage
        let breadcrumb = breadcrumb::load(&run.id, &planned.kind, work_dir);

        // Record result
        run.record(planned, &result);

        // Inject breadcrumb into next stage
        if let Some(bc) = breadcrumb {
            if let Some(next) = plan.stages.get(i + 1) {
                planner::prepend_context(next, &bc);
            }
        }

        // Validation (between stages, not after the last one)
        if result.success && i < plan.stages.len() - 1 {
            if let Err(e) = validation::run(config, &work.route, work_dir).await {
                run.record_failed(planned, &format!("validation: {e}"));
                break;
            }
        }

        if !result.success && !planned.optional {
            break;
        }
    }

    // 5. Release lease (remove in-progress, add complete/failed)
    lifecycle::release(&work.subject, &run, config, gh).await;

    // 6. Cleanup worktree
    if let Some(wt) = worktree {
        isolation::remove(repo_dir, &wt, git).await;
    }

    // 7. Persist run record
    run.finish();
    state::save(&run, state_dir);

    // 8. Notify
    notify::run_complete(config, &run).await;

    run
}
```

This replaces `process_issue_with_overrides`, `process_pr_with_overrides`, and
`process_reactive_pr` — three functions with ~800 lines total — with one ~80 line
function.

### Issue vs PR: where the differences live

| Concern | Issue | PR |
|---------|-------|-----|
| **Prompt generation** | Uses issue title/body, `{issue_number}` | Uses PR title/body, `{pr_number}`, `{head_branch}` |
| **Open PR stage** | Creates a new PR | Updates existing PR body |
| **Merge stage** | Merges the created PR | Merges the existing PR |
| **Label API** | `add_label` / `remove_label` | `add_pr_label` / `remove_pr_label` |
| **Branch** | Generated from pattern | Already exists (PR head) |
| **Env vars** | `FORZA_ISSUE_NUMBER` | `FORZA_PR_NUMBER` |

These differences are handled by:
- `planner::create` checking `subject.kind` when generating prompts
- `lifecycle` module dispatching to the right label API based on `subject.kind`
- `shell::execute` setting the right env var based on `subject.kind`
- The `OpenPr` stage handler checking whether a PR already exists

No separate code paths. Data, not control flow.

### Scheduler

The batch/poll loop becomes a simple scheduler:

```rust
pub async fn poll_cycle(
    repo: &str,
    config: &Config,
    routes: &IndexMap<String, Route>,
    scheduler: &mut Scheduler,
    gh: &dyn GitHubClient,
    git: &dyn GitClient,
    agent: &dyn AgentExecutor,
) -> Vec<Run> {
    // 1. Discover eligible work
    let candidates = discover(repo, config, routes, gh).await;

    // 2. Filter by concurrency limits
    for (subject, route_name) in candidates {
        let route = &routes[&route_name];
        let workflow = config.resolve_workflow(&route.workflow);

        let work = MatchedWork { subject, route_name, route, workflow };

        if scheduler.can_start(&work) {
            scheduler.spawn(work, config, gh, git, agent);
        } else {
            scheduler.defer(work);
        }
    }

    // 3. Collect completed runs
    scheduler.collect_completed().await
}
```

The scheduler tracks:
- Global active count vs `max_concurrency`
- Per-route active count vs `route.concurrency`
- Deferred work for the next cycle
- Active task handles (`JoinSet`)

## Crate Structure

```
forza-core/
  src/
    lib.rs              — public API
    config.rs           — RunnerConfig, Route, GlobalConfig, SecurityConfig
    workflow.rs          — Workflow, Stage, StageKind, builtin templates
    subject.rs          — Subject, SubjectKind
    condition.rs        — RouteCondition, condition evaluation
    plan.rs             — prompt generation, PlannedStage
    execute.rs          — pipeline::execute (the main function above)
    discover.rs         — discovery and route matching
    scheduler.rs        — concurrency management
    lifecycle.rs        — label management (acquire/release)
    isolation.rs        — worktree create/remove/cleanup
    hooks.rs            — pre/post/finally hooks
    validation.rs       — validation command execution
    breadcrumb.rs       — breadcrumb read/write
    run.rs              — Run, RunStatus, Outcome, persistence
    notify.rs           — notification dispatch
    shell.rs            — shell command execution (conditions, agentless)
    error.rs            — Error types
    traits.rs           — GitHubClient, GitClient, AgentExecutor traits

forza/
  src/
    main.rs             — CLI
    api.rs              — REST API
    mcp.rs              — MCP server
    github/
      gh_cli.rs         — gh CLI backend
      octocrab.rs       — octocrab backend
    git/
      cli.rs            — git CLI backend
      gix.rs            — gix backend
    agent/
      claude.rs         — Claude Code adapter
    prompts/
      *.md              — prompt templates
```

## Config Changes

### Remove global behavior flags

`auto_merge` and `draft_pr` move from global config into workflow templates:

```toml
# Before (implicit behavior)
[global]
auto_merge = true
draft_pr = true

# After (explicit stages)
[[workflow_templates]]
name = "bug"
stages = [
  { kind = "plan" },
  { kind = "draft_pr" },           # explicit: create draft after plan
  { kind = "implement" },
  { kind = "test" },
  { kind = "review" },
  { kind = "open_pr" },            # promotes draft to ready
  { kind = "merge", agentless = true,
    command = "gh pr merge $FORZA_PR_NUMBER --squash" },
]
```

If you don't want draft PRs, don't include the `draft_pr` stage. If you don't want
auto-merge, don't include the `merge` stage. The config IS the behavior.

### Reactive mode → multiple routes

```toml
# Before
[routes.auto-maintain]
condition = "any_actionable"
workflow = "pr-maintenance"    # reactive workflow with internal dispatch

# After
[routes.auto-rebase]
condition = "has_conflicts"
workflow = "pr-rebase"         # linear: revise_pr

[routes.auto-fix-ci]
condition = "ci_failing"
workflow = "pr-fix-ci"         # linear: fix_ci

[routes.auto-merge]
condition = "ci_green_no_objections"
workflow = "pr-merge"          # linear: merge
```

### needs_worktree

Some workflows (like `pr-merge`) don't need a worktree — they just run a shell
command. Add `needs_worktree` (default: true) to workflow templates:

```toml
[[workflow_templates]]
name = "pr-merge"
needs_worktree = false
stages = [
  { kind = "merge", agentless = true,
    command = "gh pr merge $FORZA_PR_NUMBER --squash" },
]
```

## Environment Variables

All shell commands (agentless stages, conditions, hooks, validation) get:

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

No more "this env var is set in the reactive path but not the linear path."

## AgentExecutor Trait

Decouples the execution engine from Claude specifically:

```rust
#[async_trait]
pub trait AgentExecutor: Send + Sync {
    async fn execute(
        &self,
        stage: &PlannedStage,
        work_dir: &Path,
        config: &Config,
        route: &Route,
    ) -> StageResult;
}
```

The `ClaudeAdapter` implements this. But you could also implement it for a mock
(testing), a different LLM, or a human-in-the-loop executor.

## Migration Path

Each step is independently deployable. No big-bang rewrite.

### Phase 1: Create forza-core, move abstractions

- Create workspace with `forza-core` crate
- Move: config, workflow, stage, subject, condition, run, error
- Move: isolation, hooks, validation, breadcrumb, notify, shell
- Move: GitHubClient, GitClient traits
- No behavior changes. Binary crate re-exports from core.

### Phase 2: Unify execution path

- Implement `pipeline::execute` in forza-core
- Implement `discover` and `Scheduler`
- Replace `process_issue_with_overrides` and `process_pr_with_overrides` with
  calls to `pipeline::execute`
- Delete `process_reactive_pr` (reactive mode deprecated)
- Delete `execute_stages` helper (absorbed into `pipeline::execute`)

### Phase 3: Clean up config

- Remove `auto_merge` global flag (behavior is in workflow templates)
- Remove `draft_pr` global flag (make it an explicit stage)
- Remove `WorkflowMode::Reactive` (keep the enum variant for deserialization
  compat, but warn and treat as linear)
- Add `needs_worktree` to workflow templates

### Phase 4: Polish

- Add `AgentExecutor` trait, implement for Claude
- Standardize env vars across all shell execution
- Add `forza explain` using the clean config model
- Add `forza doctor` for config validation
- Update docs, examples, README

## What We Keep

- All prompt templates (`src/prompts/*.md`)
- GitHub client implementations (gh CLI, octocrab)
- Git client implementations (CLI, gix)
- CLI structure and all subcommands
- REST API and MCP server
- Worktree management
- Notification system
- State persistence format (run records)
- All 12 stage kinds
- Label lifecycle
- Breadcrumb system
- Hook system
- Skill injection
- Config file format (mostly — remove deprecated fields)

## What We Delete

- `process_issue_with_overrides` / `process_issue_with_config`
- `process_pr_with_overrides` / `process_pr_with_config`
- `process_reactive_pr`
- `execute_stages` helper
- `build_pr_planned_stage` / `generate_reactive_pr_prompt`
- `WorkflowMode::Reactive`
- `RouteCondition::AnyActionable` (replaced by multiple focused routes)
- `auto_merge` global config field
- `draft_pr` global config field

## Success Criteria

After the refactor:

1. `grep -r "process_issue\|process_pr\|process_reactive" src/` returns zero hits
   outside of tests and deprecation warnings
2. The orchestrator module is replaced by `pipeline::execute` (~80 lines)
3. Adding a new stage kind requires changes in exactly two places: the `StageKind`
   enum and the prompt template
4. A condition route PR going through fix-ci → merge takes two poll cycles and
   produces two clean run records
5. `cargo test` passes with no new test failures
6. `forza explain` can render any config as a readable route map
