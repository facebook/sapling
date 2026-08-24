---
name: ACR-wlock-ordering
metadata:
  oncalls: ['scm_client_infra']
  strict: true
  apply_to_path: 'eden/scm/sapling/.*\.py$'
  apply_to_content: 'wlock|repo\.lock|release\(lock'
  apply_to_clients: ['code_review']
---

# Working Copy Lock Ordering

**Severity: CRITICAL**

## What to Look For

- Lock acquisitions using `repo.wlock()` and `repo.lock()`
- `release()` calls that don't release in reverse acquisition order
- Missing lock acquisitions around working copy mutations

## When to Flag

- `repo.lock()` acquired before `repo.wlock()` — must always acquire `wlock` first to prevent deadlocks
- `release(wlock, lock)` instead of `release(lock, wlock)` — locks must be released in reverse order
- Working copy mutations (writing to `.hg/`, modifying tracked files) without holding `wlock`
- Store mutations (writing to revlog, changelog) without holding `lock`
- Missing `try/finally` around lock pairs — if an exception occurs between acquisition, the second lock leaks

## Do NOT Flag

- `with repo.wlock():` context manager usage — handles release automatically
- `with repo.wlock(), repo.lock(), repo.transaction("name") as tr:` — correct combined pattern
- Code that only reads repository state (no mutations) without holding locks
- Lock acquisitions in test code using `testutil` fixtures

## Examples

**BAD (store lock before wlock — deadlock risk):**
```python
lock = repo.lock()       # WRONG: store lock first
wlock = repo.wlock()     # another thread may hold wlock and want lock
try:
    # ... mutate working copy and store ...
finally:
    release(lock, wlock)  # also wrong release order
```

**GOOD (wlock before lock, release in reverse):**
```python
wlock = repo.wlock()
lock = repo.lock()
try:
    # ... mutate working copy and store ...
finally:
    release(lock, wlock)  # release store lock first, then wlock
```

**ALSO GOOD (context manager):**
```python
with repo.wlock(), repo.lock(), repo.transaction("myoperation") as tr:
    # ... mutate working copy and store ...
    # locks released automatically in correct order
```

Note: Python's `with a(), b():` context manager guarantees `b.__exit__()` is called before `a.__exit__()` — release order is the reverse of acquisition order, which is correct.

**BAD (mutation without wlock):**
```python
def update_dirstate(repo, node):
    # WRONG: no wlock held — concurrent operations may corrupt dirstate
    repo.dirstate.setparents(node)
    repo.dirstate.write()
```

**GOOD (mutation protected by wlock):**
```python
def update_dirstate(repo, node):
    with repo.wlock():
        repo.dirstate.setparents(node)
        repo.dirstate.write()
```
