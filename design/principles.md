# forza — Design Principles

This document formalizes what forza is and is not responsible for. It is the reference
point when evaluating new features. If a proposal crosses a boundary, it needs strong
justification.

## Three actors, clear lanes

### Forza decides (the process)

- Which stages run and in what order
- When to stop (validation failure, retry budget)
- Where work happens (worktrees, branches)
- What labels to apply and when
- How to recover (condition routes, retry with backoff)

### Humans decide (the direction)

- What to work on (labeling issues `forza:ready`)
- When to start (manual run, watch mode, or action trigger)
- What "done" looks like (acceptance criteria in the issue)
- Whether to merge (or let auto-merge handle it)
- Sequencing and prioritization of work

### The agent decides (the implementation)

- How to implement the plan
- What files to change
- How to fix failures
- What to write in the PR

## Key principle

Forza is infrastructure, not an autonomous agent. It provides deterministic guardrails —
stages, validation, lifecycle labels, retry budgets — and the human decides what to work
on and when. The agent decides how. Adding intelligence to the framework adds
unpredictability.

## Evaluating features

When a feature is proposed, ask:

- Does this add decision-making to forza that belongs with the human or the agent?
- Does this make the pipeline less deterministic?
- Can this be achieved by a better issue, a better prompt, or a better config instead
  of new framework logic?

If the answer to any of these is "yes," the feature probably does not belong in forza.

### Examples

**Belongs in forza:** A `max_retries` field that stops a route after N failures. This
is process control — a guardrail the human configures.

**Does not belong in forza:** Automatically adjusting the prompt when a stage fails
repeatedly. That is intelligence in the framework. The human should update the issue;
the agent should handle variation.

**Belongs in forza:** A `condition` field on a stage that gates execution on a shell
command exit code. The human writes the condition; forza enforces it.

**Does not belong in forza:** Automatically deciding which workflow to use based on
issue content analysis. Route matching is the human's job; it must be explicit in config.

## Design principles

These follow directly from the three-actor model.

1. **One route, one action, one cycle.** A route does exactly one thing. If a PR needs
   rebasing and CI fixing and merging, that is three routes across three poll cycles.
   Multi-step behavior is achieved through configuration, not framework complexity.

2. **GitHub is the state machine.** PR state on GitHub is authoritative. Routes are
   transition functions. The poll loop is the event loop. Forza does not maintain an
   internal state machine that duplicates or diverges from GitHub state.

3. **Match once, carry through.** Once a subject is matched to a route, that binding is
   immutable for the duration of the run. No re-matching mid-execution. The human's
   intent, expressed as a label or PR state, is locked in at discovery time.

4. **Config is behavior.** Everything a route does is visible in `forza.toml`. No global
   flags that secretly modify stage behavior. If you want auto-merge, include the merge
   stage. If you do not want it, leave it out.

5. **One path through the code.** Issues and PRs follow the same execution pipeline.
   Differences are data, not control flow. A unified pipeline is easier to reason about,
   test, and extend.

## What forza isn't

**forza will fail, and that's by design.** When a run fails, forza stops, labels the
issue, and tells you what happened. It doesn't retry, work around, or guess. The right
response is usually to read the failure, improve the issue description, and run again.

- **Not fully autonomous** — humans decide what to work on and when.
- **Not self-healing** — failures are reported, not automatically resolved.
- **Not a replacement for good issue writing** — vague issues produce vague results.
- **Not an agent** — forza is infrastructure that agents run inside.
- **Not trying to handle every edge case** — simplicity and determinism over cleverness.

The temptation is to make forza handle more. Resist it. Every conditional path added to
the pipeline is complexity that makes the system harder to reason about. If something
fails, the answer is usually a better issue, a better prompt, or a human decision — not
more framework logic.

## What this rules out

- **Adaptive prompting**: forza does not modify prompts based on prior failures. The
  agent adapts; forza does not.
- **Automatic workflow selection**: the human picks the workflow via route config. Forza
  does not infer it from issue content.
- **In-process decisions between stages**: forza does not inspect stage output to decide
  what to do next. The stage sequence is fixed in config.
- **Autonomous issue triage**: forza does not label, prioritize, or select which issues
  to work on. It only acts on issues that already have the gate label.
- **Framework-level retry intelligence**: retries are a counter, not a strategy. Forza
  retries blindly up to `max_retries`; the agent is responsible for varying its approach.
