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

This follows directly from the [three-actor model](overview.md#three-actors-clear-lanes): forza controls process, humans control direction, agents control implementation. Blurring those lanes adds unpredictability without adding value.

## What this rules out

- **Adaptive prompting** — forza does not modify prompts based on prior failures. The agent adapts; forza does not.
- **Automatic workflow selection** — the human picks the workflow via route config. Forza does not infer it from issue content.
- **In-process decisions between stages** — forza does not inspect stage output to decide what to do next. The stage sequence is fixed in config.
- **Autonomous issue triage** — forza does not label, prioritize, or select which issues to work on. It only acts on issues that already have the gate label.
- **Framework-level retry intelligence** — retries are a counter, not a strategy. Forza retries blindly up to `max_retries`; the agent is responsible for varying its approach.

## Evaluating features

When a feature is proposed, ask:

- Does this add decision-making to forza that belongs with the human or the agent?
- Does this make the pipeline less deterministic?
- Can this be achieved by a better issue, a better prompt, or a better config instead of new framework logic?

If the answer to any of these is "yes," the feature probably does not belong in forza.

### Examples

**Belongs in forza:** A `max_retries` field that stops a route after N failures. This is process control — a guardrail the human configures.

**Does not belong in forza:** Automatically adjusting the prompt when a stage fails repeatedly. That is intelligence in the framework. The human should update the issue; the agent should handle variation.

**Belongs in forza:** A `condition` field on a stage that gates execution on a shell command exit code. The human writes the condition; forza enforces it.

**Does not belong in forza:** Automatically deciding which workflow to use based on issue content analysis. Route matching is the human's job; it must be explicit in config.
