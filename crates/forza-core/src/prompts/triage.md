{preamble}

You are triaging GitHub issues for **{repo}**.

Your job is to analyze the issues below, then create a single **plan issue** that
organizes them for automated processing by forza.

## Steps

1. Read `CLAUDE.md`, `README.md`, and relevant source files to understand the project.
2. Analyze each issue: read the code it references, understand scope and complexity.
3. Classify each issue into one of the configured routes below.
4. Assess readiness: can it be worked now, or is something blocking it?
5. Detect dependencies between issues (shared code, ordering constraints, explicit references).
6. Create the plan issue with the structure described below.
7. For any blocked issues, post a comment on the original issue explaining why it is blocked,
   and add the `forza:needs-human` label.

## Configured routes

{routes}

## Issues to triage

{issues}

## Plan issue format

Create the plan issue using:

```
gh issue create \
  --repo {repo} \
  --title "forza: triage plan for {issue_refs}" \
  --label forza:plan \
  --body "$(cat <<'PLAN_EOF'
<plan body>
PLAN_EOF
)"
```

The plan body must follow this structure:

```markdown
# Triage Plan

Triaged N issues on YYYY-MM-DD.

## Actionable

Issues ready for automated processing, listed in recommended implementation order.
Earlier items should be completed before later items when there are dependencies.
Issues with no dependencies between them can be processed in parallel.

### 1. #N -- <issue title>
**Route**: <route name>
**Context**: <1-3 sentences: what needs to happen, relevant files, why this ordering>

### 2. #M -- <issue title>
**Route**: <route name>
**Context**: <1-3 sentences>

## Blocked

Issues that cannot be processed automatically right now.

### #X -- <issue title>
**Reason**: <needs-human | needs-research | too-large | unclear-requirements>
**Details**: <what is missing or unclear, what the human needs to decide>

## Skipped

Issues excluded from this triage (already processed, in progress, etc).

- #Y -- <reason>
```

## Blocked issue handling

For each issue in the Blocked section, also:

1. Add `forza:needs-human` label:
   `gh issue edit --repo {repo} <number> --add-label forza:needs-human`

2. Post a comment explaining why:
   ```
   gh issue comment --repo {repo} <number> --body "$(cat <<'COMMENT_EOF'
   **Triage note**: This issue was reviewed during triage but cannot be processed automatically.

   **Reason**: <reason>

   **What's needed**: <specific action items for the human>
   COMMENT_EOF
   )"
   ```

## Guidelines

- Be specific in Context fields -- name files, functions, modules.
- Order actionable issues so dependencies come first.
- If two issues modify the same files, note the ordering constraint.
- An issue that is too large for a single agent workflow should go in Blocked with
  reason `too-large` and suggestions for how to split it.
- Do NOT modify actionable issues (no labels, no comments). The plan is the output.
- Do NOT add route labels to issues. That happens during execution, not triage.
