# Writing Issues for Forza

Forza implements issues autonomously. The quality of the issue directly affects the quality of the result. An issue that is clear, scoped, and includes acceptance criteria will produce a better PR than a vague one.

## What makes a good forza issue

**Title** — state the change, not the symptom. `fix: panic when route label is missing` is better than `route matching broken`.

**Description** — provide enough context for an agent with no prior knowledge of the problem. Include:

- What the current behavior is
- What the expected behavior is
- Any relevant config, error output, or stack traces

**Acceptance criteria** — the most important field. A bulleted checklist of specific, testable conditions. Forza uses these directly to determine when the work is done.

```
- [ ] Running `forza watch` with a missing label no longer panics
- [ ] A descriptive error message is logged instead
- [ ] Existing label-matching behavior is unchanged
```

Without acceptance criteria, forza must guess what "done" means. With them, it knows exactly what to implement and verify.

**Affected files** — optional but valuable. If you know which source files are involved, list them. Forza will focus there rather than searching the whole codebase.

```
src/config.rs
crates/forza/src/runner.rs
```

## Issue types and what to include

| Type | Key fields |
|------|-----------|
| Bug | Description of current vs. expected behavior, steps to reproduce, acceptance criteria |
| Feature | Use case and motivation, acceptance criteria, implementation notes if any |
| Chore | What to change and why, acceptance criteria |
| Research | Question to answer, scope boundaries, expected output format (comment, doc, PR) |
| Docs | Which doc or section, what is missing or wrong, acceptance criteria |

## Tips

- **Scope narrowly.** One issue, one change. Forza runs each issue end-to-end in a worktree; a tightly scoped issue produces a focused, reviewable PR.
- **Avoid implementation instructions in bugs.** Describe the problem, not the fix. Let the plan stage decide the approach.
- **Use implementation notes for features.** If you have a preferred approach or design constraint, put it in the description so forza considers it during planning.
- **Include the `forza:ready` label last.** Fill in the issue completely before applying the gate label. Once labeled, forza picks it up immediately.

## Examples

### Bug — well-formed

> **Title:** `fix: watch exits silently when gh CLI returns rate limit error`
>
> **Description:** When `gh` hits the API rate limit, `forza watch` exits without printing an error. The process just stops.
>
> **Acceptance criteria:**
> - [ ] Rate limit errors from `gh` are detected and logged as warnings
> - [ ] `forza watch` retries after a backoff period instead of exiting
> - [ ] Normal watch behavior is unchanged
>
> **Affected files:** `crates/forza/src/runner.rs`, `src/github.rs`

### Feature — well-formed

> **Title:** `feat: add schedule window support to routes`
>
> **Description:** I want to restrict certain routes to run only during business hours (e.g., 09:00–17:00 UTC) to avoid noisy notifications at night.
>
> **Acceptance criteria:**
> - [ ] Routes accept an optional `schedule` field with a cron-style time window
> - [ ] Issues outside the window are skipped and retried on the next poll inside the window
> - [ ] Routes without a `schedule` field behave exactly as today
>
> **Implementation notes:** A simple `start_hour`/`end_hour` UTC range is sufficient for now; full cron syntax is not needed.

### Vague — will produce poor results

> fix the route stuff it keeps breaking

Forza will attempt to implement this, but without a description of what is broken or what "fixed" means, the result is unpredictable.
