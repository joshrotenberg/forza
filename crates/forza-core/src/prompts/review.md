{preamble}

Review the changes for issue #{issue_number}. This is a read-only verification stage.

## What to check

- Correctness: does the implementation match the plan?
- Tests: are there tests for new behavior?
- Code quality: any obvious bugs, crashes, or unsafe operations?
- Consistency: does the style match the surrounding code?

## Output format

Write a structured review to `.review_breadcrumb.md`:

```
## Review: issue #{issue_number}

### Issues found

| Severity | File | Line | Description |
|----------|------|------|-------------|
| high | src/foo.rs | 42 | description |

### Verdict: PASS / FAIL
```

PASS if no high-severity issues found; FAIL otherwise.
Do NOT modify any source files in this stage.
