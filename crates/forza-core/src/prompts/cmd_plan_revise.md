{preamble}

You are revising an existing triage plan for **{repo}**.

A human has reviewed the plan and left comments. Read the original plan, the comments,
and the codebase, then update the plan issue to reflect the feedback.

## Current plan (issue #{plan_number})

{plan_body}

## Comments from human review

{comments}

## Instructions

1. Read the comments carefully. The human may be:
   - Correcting a classification (e.g., "this is a bug, not a feature")
   - Reordering priorities (e.g., "#45 should come before #42")
   - Providing missing context that unblocks an issue
   - Asking you to split or combine issues
   - Removing issues from the plan
   - Adding new issues to evaluate

2. If comments reference new issues or ask you to re-evaluate, read the relevant issues
   and code before updating the plan.

3. Update the plan issue body with the revised plan:
   ```
   gh issue edit --repo {repo} {plan_number} --body "$(cat <<'PLAN_EOF'
   <revised plan body>
   PLAN_EOF
   )"
   ```

4. If an issue has moved from Blocked to Actionable, remove `forza:needs-human`:
   `gh issue edit --repo {repo} <number> --remove-label forza:needs-human`

5. If an issue has moved from Actionable to Blocked, add `forza:needs-human` and
   post a comment on the original issue explaining why.

6. Post a brief comment on the plan issue summarizing what changed:
   ```
   gh issue comment --repo {repo} {plan_number} --body "$(cat <<'COMMENT_EOF'
   **Plan revised.**

   Changes:
   - <bullet list of what changed and why>
   COMMENT_EOF
   )"
   ```

## Guidelines

- Preserve the same plan structure (Actionable / Blocked / Skipped sections).
- Keep the plan title unchanged.
- Be specific about what changed and why in the summary comment.
- If the human's feedback is unclear, note the ambiguity in the plan rather than guessing.
