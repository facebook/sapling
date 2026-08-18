---
name: ACR-abort-without-hint
metadata:
  oncalls: ['scm_client_infra']
  strict: true
  apply_to_path: 'eden/scm/sapling/.*\.py$'
  apply_to_content: 'error\.Abort|raise.*Abort'
  apply_to_clients: ['code_review']
---

# Abort Without User Hint

**Severity: LOW**

## What to Look For

- `raise error.Abort(...)` calls that don't include a `hint=` parameter
- Error messages that tell the user what went wrong but not what to do about it

## When to Flag

**IMPORTANT: Only flag NEW `error.Abort` calls introduced in the diff. Do not flag existing calls being moved or refactored.**

- `error.Abort(_("message"))` without `hint=` when a reasonable user action exists (e.g., "use --force", "run sl doctor", "check your config")
- New `error.Abort` calls in user-facing command paths that provide no guidance
- When in doubt, don't flag — only flag when you can write a specific, actionable hint that would genuinely help a non-expert user

## Do NOT Flag

- `error.Abort` in internal/developer-facing code paths where the user can't take action
- `error.Abort` where the error message itself contains the remediation (e.g., "run 'sl checkout' first")
- `error.ProgrammingError` — these are developer bugs, not user errors
- Existing `error.Abort` calls without hints — only flag new ones in the diff
- `error.Abort` for truly unrecoverable situations where no hint would help
- `error.Abort` in argument/flag validation where the error message makes the fix self-evident (e.g., `"cannot use --X with --Y"`, `"at least one argument required"`)
- `error.Abort` calls that are being moved or refactored, not newly introduced

## Examples

**BAD (error about failed operation with no guidance):**
```python
if not repo.revs("ancestors(.) & %s", dest):
    raise error.Abort(_("cannot rebase to destination"))
```

**GOOD (hint tells user what to do):**
```python
if not repo.revs("ancestors(.) & %s", dest):
    raise error.Abort(
        _("cannot rebase to destination"),
        hint=_("use 'sl rebase -d <dest>' with a valid destination, or try 'sl pull' first"),
    )
```

**BAD (error about missing config with no guidance):**
```python
if not ui.config("paths", "default"):
    raise error.Abort(_("no default repository path configured"))
```

**GOOD (actionable hint):**
```python
if not ui.config("paths", "default"):
    raise error.Abort(
        _("no default repository path configured"),
        hint=_("set paths.default in your repo config or pass --repository"),
    )
```
