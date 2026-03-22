# Tricky Bug Breakdown

Three independent bugs that interact to create hard-to-diagnose reactive route failures.

---

## Bug 1: `RouteCondition::matches()` doesn't skip when mergeable state is unresolved

**File:** `src/config.rs` — `RouteCondition::matches()`

The method logs "mergeability not yet resolved, skipping cycle" when `mergeable` is
`UNKNOWN` or `None`, but never actually returns `false`. The debug log is aspirational —
execution falls through to the condition evaluation where `has_conflicts` is false (since
it's not `CONFLICTING`), so `!has_conflicts` is true. This lets `CiGreenNoObjections`
and `ApprovedAndGreen` match PRs whose mergeability GitHub hasn't computed yet.

**Impact:** Premature dispatch into `process_reactive_pr`. The PR gets the in-progress
label, a worktree is created, but the shell-based stage conditions (which query GitHub
live) see the real unresolved state. No stage condition matches, the run finishes as
`NothingToDo`, the label is removed, and the PR appears stuck. On the next poll cycle,
the same thing happens again — an infinite loop of no-op cycles.

**Fix:** Return `false` early when `mergeable` is not `MERGEABLE` or `CONFLICTING`:

```rust
// src/config.rs, inside RouteCondition::matches()
if let Some(m) = pr.mergeable.as_deref() {
    if m != "MERGEABLE" && m != "CONFLICTING" {
        debug!(pr = pr.number, mergeable = m, "mergeability not yet resolved, skipping cycle");
        return false;
    }
} else {
    debug!(pr = pr.number, mergeable = "None", "mergeability not yet resolved, skipping cycle");
    return false;
}
```

**Tests:** Existing tests cover `UNKNOWN` and `None` mergeable states for `HasConflicts`
and `CiFailingOrConflicts` but don't assert that `ApprovedAndGreen` or
`CiGreenNoObjections` return false for those states. Add test cases confirming all
condition variants return false when mergeability is unresolved.

---

## Bug 2: `checks_passing()` treats unknown check conclusions as indeterminate instead of failed

**File:** `src/github/mod.rs` — `checks_passing()`

The catch-all branch (`_ => all_concluded = false`) for check conclusions not in the
hardcoded list causes the function to return `None` (indeterminate) instead of
`Some(false)` (failing).

The known-good conclusions are `SUCCESS`, `SKIPPED`, and `NEUTRAL`. The known-bad ones
are `FAILURE`, `TIMED_OUT`, `CANCELLED`, and `ACTION_REQUIRED`. But GitHub can return
other values like `STARTUP_FAILURE` or `STALE`, and new values could be added in the
future. The current logic treats these as "not yet concluded," which puts the PR in a
dead zone where `ci_green` (`checks_passing == Some(true)`) and `ci_failing`
(`checks_passing == Some(false)`) are both false. Neither `auto-fix` nor `auto-merge`
will ever match the PR — it's stuck permanently until a human intervenes.

**Fix:** Invert the logic — treat anything that isn't a known-good conclusion as either a
failure or pending:

```rust
fn checks_passing(rollup: &[GhStatusCheck]) -> Option<bool> {
    if rollup.is_empty() {
        return None;
    }
    let mut all_concluded = true;
    for check in rollup {
        match check.conclusion.as_deref() {
            Some("SUCCESS") | Some("SKIPPED") | Some("NEUTRAL") => {}
            None => all_concluded = false,  // genuinely pending, no conclusion yet
            Some(_) => return Some(false),  // any other conclusion is a failure
        }
    }
    if all_concluded { Some(true) } else { None }
}
```

Only `None` conclusion (truly in-progress, no result yet) is treated as indeterminate.
Any concluded check that isn't in the success set is a failure.

**Tests:** Add test cases for `STARTUP_FAILURE`, `STALE`, and an unknown future value to
confirm they produce `Some(false)`.

---

## Bug 3: Consolidate `auto-fix` and `auto-merge` condition routes to prevent ping-pong (ref #235)

**File:** `forza.toml`

The current config has two condition routes targeting the same PRs with the same workflow:

```toml
[repos."joshrotenberg/forza".routes.auto-fix]
condition = "ci_failing_or_conflicts"
workflow = "pr-maintenance"

[repos."joshrotenberg/forza".routes.auto-merge]
condition = "ci_green_no_objections"
workflow = "pr-maintenance"
```

Since `pr-maintenance` is a reactive workflow with its own prioritized stage conditions
(conflicts → CI failing → changes requested → merge), the route-level condition is
redundant. Both routes dispatch into the same reactive loop that re-evaluates PR state
via shell commands before picking a stage.

**Problems:**

1. After `auto-fix` resolves an issue and CI re-runs, `checks_passing` returns `None`
   during the pending window. Neither route matches, so the PR sits idle until the next
   poll cycle after CI finishes.
2. Both routes can evaluate the same PR in the same poll cycle if the PR state is
   ambiguous, potentially spawning duplicate `process_reactive_pr` calls.
3. The `max_retries` budget is tracked per-route, so a PR that bounces between routes
   gets 6 total attempts (3 per route) instead of the intended 3.

**Fix options:**

- **Option A:** Add a new `RouteCondition` variant (e.g., `NeedsAttention` or `Any`)
  that matches any PR not in a terminal state. The reactive workflow's stage conditions
  handle dispatch.
- **Option B:** Remove the route-level condition requirement for reactive workflows
  entirely. If the workflow is reactive, fetch all in-scope PRs and let the stage
  conditions decide. The route condition becomes an optional pre-filter, not a gate.

Replace both routes with a single route:

```toml
[repos."joshrotenberg/forza".routes.auto-maintain]
type = "pr"
condition = "any_actionable"   # new variant, or use option B
workflow = "pr-maintenance"
scope = "forza_owned"
max_retries = 3
concurrency = 2
poll_interval = 60
```
