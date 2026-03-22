{preamble}

Read issue #{issue_number} and analyze the codebase to create an implementation plan.

{issue_title}

Issue body:
{issue_context}

## Steps

1. Search for and read the relevant files — do not guess at file locations.
2. Understand the current architecture and patterns used.
3. Identify exactly which files need to change and why.

## Breadcrumb

Write the plan to `.plan_breadcrumb.md` in the repo root with these sections:
- **Files to modify**: list each file path, one per line
- **Approach**: 2-5 sentence summary of what will change and why
- **Commit message**: the exact conventional-commit message for the implement stage (e.g., `feat(module): short description closes #{issue_number}`)

Do NOT modify any source files. This is a planning-only stage.
