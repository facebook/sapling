---
name: bug-fix-diff-structure
metadata:
  oncalls: ['scm_server_infra']
  strict: true
  apply_to_user_prompt: 'fix.{0,20}bug|bug.{0,20}fix|bugfix'
---

# Bug Fix Diff Structure

**Severity: MEDIUM**

## What to Look For

- A non-trivial bug fix that changes code and tests in the same diff without first demonstrating the broken behavior

## Do NOT Flag

- Small, obvious fixes where the correctness is clear from reading the diff (typos, one-liners, straightforward few-line changes)
- Fixes where existing tests already cover the broken behavior and just need assertion updates
- When already working on a diff stack that follows this pattern
- Fixes to config, logging, metrics, or other areas where the bug is not reproducible in unit or integration tests

## Recommendation

For non-trivial bug fixes where the behavior is testable, consider a two-diff stack:

1. **First diff**: Add a test asserting the current broken behavior with a `FIXME` comment.
2. **Second diff**: Fix the bug and update the test, removing the `FIXME`.

Both diffs should pass tests independently. This makes the bug visible in review and proves the fix actually changes behavior.
