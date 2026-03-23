# What forza isn't

**forza will fail, and that's by design.** When a run fails, forza stops, labels the issue, and tells you what happened. It doesn't retry, work around, or guess. The right response is usually to read the failure, improve the issue description, and run again.

## What forza is not

- **Not fully autonomous** — humans decide what to work on and when. Labeling an issue `forza:ready` is a human decision.
- **Not self-healing** — failures are reported, not automatically resolved. A failed stage means a labeled issue and a failure comment, not a retry loop.
- **Not a replacement for good issue writing** — vague issues produce vague results. The quality of forza's output is bounded by the quality of the issue.
- **Not an agent** — forza is infrastructure that agents run inside. The agent implements; forza orchestrates.
- **Not trying to handle every edge case** — simplicity and determinism over cleverness. If an edge case isn't covered, the answer is usually a better issue or a human decision.

## Why this matters

The temptation is to make forza handle more. Resist it. Every conditional path added to the pipeline is complexity that makes the system harder to reason about. If something fails, the answer is usually a better issue, a better prompt, or a human decision — not more framework logic.

This follows directly from the three-actor model: forza controls process, humans control direction, agents control implementation. Blurring those lanes adds unpredictability without adding value.

See [design/principles.md](https://github.com/joshrotenberg/forza/blob/main/design/principles.md) in the repository for the full design rationale.
