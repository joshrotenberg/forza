# Workflow Patterns

Practical patterns for using forza day-to-day. Not prescriptive — here's how it tends to work well in practice.

## Background worker + foreground agent

The primary pattern. Run `forza watch` in one terminal while you work with an agent (Claude Code, Codex) in another. The agent files issues, you label them ready, forza works them in the background.

```
Terminal 1: forza watch --config forza.toml
Terminal 2: claude  (your interactive session)
```

The flow:

1. You're working on a feature or talking through a design with your agent
2. The agent identifies something that needs code changes — it files a GitHub issue
3. You review the issue, add acceptance criteria if needed, apply `forza:ready`
4. Forza picks it up on the next poll, runs the workflow, opens a PR
5. You review the PR; if the agent missed something, you update the issue description and re-apply `forza:ready`

Failures are expected and fine. When a run fails, forza posts a comment on the issue explaining what went wrong. Read the comment, adjust the issue if the description was unclear, and re-mark it ready. Use `forza fix` to re-run with the failure as context if the problem was transient.

If you want to expose the REST API to your agent (for triggering runs directly or querying status):

```
forza watch --serve-api
```

## Targeted single runs

When you know exactly what needs to happen, skip the label dance and point forza directly at an issue.

```bash
# Preview: see which route matches and what stages will run
forza issue 42 --dry-run

# Do the work
forza issue 42
```

Good for:

- Verifying your config is right before setting up watch mode
- Re-running a specific issue after you've edited its description
- One-off automation where continuous polling isn't needed

The `--dry-run` flag is especially useful when introducing a new route — you can confirm the match and planned stages before committing to execution.

## Batch processing

File a set of issues, label a wave, let forza work through them. Label the next wave when the first batch merges.

```bash
# Phase 1: label the first set
gh issue edit 10 --add-label forza:ready
gh issue edit 11 --add-label forza:ready
gh issue edit 12 --add-label forza:ready

# forza watch picks them up; wait for PRs to merge

# Phase 2: label the next set
gh issue edit 20 --add-label forza:ready
gh issue edit 21 --add-label forza:ready
```

Waves work well because forza creates a worktree per issue. Running too many in parallel against the same files can produce conflicting PRs. A conservative approach: label 2–4 issues at a time, wait for PRs to land, then label the next group.

For a cron-driven setup instead of continuous watch:

```bash
# Run one batch cycle from a cron job or CI
forza run
```

## New project bootstrap

Use `forza init` to set up config, then have your agent turn a design doc into issues, and process them in waves.

```bash
# Create labels and generate a starter config
forza init --repo owner/name

# Optional: guided init walks you through config setup interactively
forza init --guided
```

The guided init asks about your repo, preferred agent, and security settings, then writes a `forza.toml`. It also offers to create a test issue so you can verify the setup works end-to-end before committing to real work.

From there:

1. Hand your agent a design doc or list of requirements
2. Agent creates GitHub issues for each discrete chunk of work
3. You review and scope the issues (narrow ones produce better PRs)
4. Apply `forza:ready` to the first wave
5. Review PRs as they come in; label the next wave

The key is keeping issues small and focused. One issue, one PR. Forza runs each issue end-to-end in its own worktree — a tightly scoped issue produces a focused, reviewable PR.

## Multi-repo management

Configure multiple repos in one `forza.toml` and run a single watch process to handle all of them.

```toml
[global]
agent = "claude"
model = "claude-sonnet-4-6"

[repos."org/service-a"]
[repos."org/service-a".routes.bugfix]
type = "issue"
label = "bug"
workflow = "bug"

[repos."org/service-b"]
[repos."org/service-b".routes.chore]
type = "issue"
label = "chore"
workflow = "chore"

[repos."org/service-b".routes.auto-rebase]
type = "pr"
condition = "has_conflicts"
scope = "forza_owned"
workflow = "pr-rebase"
```

One `forza watch` handles all configured repos. Routes are per-repo, so different repos can have different workflows, gate labels, and PR handling.

Use `forza explain` to verify routing across repos before running:

```bash
forza explain --issues     # show all issue routes
forza explain --prs        # show all PR routes
```

## Handling failures

Failures are part of the workflow, not exceptions to it.

When a run fails:

1. Forza posts a comment on the issue with the failure reason and which stage failed
2. Check the comment — usually it's a missing acceptance criterion, an ambiguous description, or a validation failure
3. Edit the issue to clarify, then re-apply `forza:ready`

For transient failures (flaky tests, network hiccups):

```bash
# Re-run the failed run with the error as context
forza fix --run-id <id>

# Or just re-label the issue
```

For persistent failures, `forza status` shows recent run history with outcomes:

```bash
forza status --limit 20
```

A run that fails the same stage repeatedly usually means the issue description needs more specificity, or the relevant acceptance criteria are missing.
