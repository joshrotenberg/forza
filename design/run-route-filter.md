# Route filtering for `forza run`

## Summary

Add a `--route` flag to `forza run` that filters discovery to a specific route,
matching the existing `--route` flag on `forza watch`.

## Current state

| Command | `--route` support | Behavior |
|---------|-------------------|----------|
| `forza watch` | Yes | Only evaluates the named route each poll cycle |
| `forza run` | No | Evaluates all routes in a single batch cycle |
| `forza pr` | Yes (override) | Forces a specific route for the given PR |
| `forza issue` | No (matches by label) | Matches route by issue labels |

## Proposal

Add `--route <name>` to `forza run`. When provided, discovery only evaluates the
named route — all other routes are skipped. When omitted, behavior is unchanged
(all routes evaluated).

### CLI

```
forza run --route bugfix          # only process issues matching the bugfix route
forza run --route auto-merge      # only evaluate the auto-merge condition route
forza run --route auto-fix-ci --route auto-rebase   # multiple routes (future)
```

### Implementation

The change is a filter on the routes passed to `runner::process_batch`. In
`cmd_run`, after resolving repos and routes:

```rust
// Before calling process_batch, filter routes if --route is specified.
let routes = if let Some(ref route_name) = args.route {
    let mut filtered = IndexMap::new();
    if let Some(route) = routes.get(route_name) {
        filtered.insert(route_name.clone(), route.clone());
    } else {
        eprintln!("error: unknown route '{route_name}'");
        return ExitCode::FAILURE;
    }
    filtered
} else {
    routes.clone()
};
```

No changes to `runner.rs`, `forza-core`, or the pipeline. Discovery and execution
are unaware of the filter — they just see a smaller route table.

### Action integration

The `--route` flag composes naturally with the GitHub Action. Users can create
separate workflow files per route category:

```yaml
# .github/workflows/forza-issues.yml
name: forza (issues)
on:
  issues:
    types: [labeled]
jobs:
  forza:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: anthropics/setup-claude-code@v1
      - uses: joshrotenberg/forza-action@v1
        with:
          command: run
          args: "--route bugfix"
        env:
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
```

```yaml
# .github/workflows/forza-pr-maintenance.yml
name: forza (PR maintenance)
on:
  check_suite:
    types: [completed]
jobs:
  forza:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: anthropics/setup-claude-code@v1
      - uses: joshrotenberg/forza-action@v1
        with:
          command: run
          args: "--route auto-merge"
        env:
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
```

This gives fine-grained concurrency control — issue processing and PR maintenance
run in separate jobs with separate concurrency groups.

### Why not a separate action input?

The action could have a dedicated `route` input instead of passing it through
`args`. But `args` is simpler — it's a passthrough to forza's CLI, so any new
flag forza adds works immediately without updating the action. A dedicated input
could be added later as sugar if the pattern is common enough.

## Use cases

1. **Separate Actions workflows**: Different triggers and concurrency for issue
   routes vs PR condition routes.

2. **Debugging**: Test a single route's behavior without processing everything:
   `forza run --route bugfix`

3. **Gradual rollout**: Enable routes one at a time in production by running
   separate `forza run --route <name>` invocations.

4. **Resource isolation**: Run expensive routes (bug fixes with Opus) on beefier
   runners, cheap routes (auto-merge) on smaller ones.

## Non-goals

- **Multiple `--route` flags**: Could accept multiple routes in one invocation,
  but single-route is sufficient for v1. Multiple routes can use multiple
  invocations.

- **Route exclusion (`--skip-route`)**: Inverse of `--route`. Not needed yet —
  if you want to skip one route, name the ones you want instead.

- **Route groups**: Tagging routes and filtering by tag (e.g. `--route-group pr`).
  Over-engineering for now.
