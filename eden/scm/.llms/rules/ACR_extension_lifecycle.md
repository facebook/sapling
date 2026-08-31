---
name: ACR-extension-lifecycle
metadata:
  oncalls: ['scm_client_infra']
  strict: true
  apply_to_path: 'eden/scm/sapling/ext/.*\.py$'
  apply_to_content: 'uisetup|extsetup|reposetup|wrapfunction|wrapcommand'
  apply_to_clients: ['code_review']
---

# Extension Lifecycle Ordering

**Severity: MEDIUM**

## What to Look For

- Extension lifecycle functions: `uisetup(ui)`, `extsetup(ui)`, `reposetup(ui, repo)`
- `extensions.wrapfunction()` and `extensions.wrapcommand()` calls
- Code that assumes other extensions are loaded or that a repository is available

## When to Flag

- `extensions.wrapfunction()` or `extensions.wrapcommand()` called inside `uisetup()` when the target is from **another extension** — at this point, other extensions may not be loaded yet, so the target function/command may not exist
- `extensions.wrapfunction()` or `extensions.wrapcommand()` called inside `reposetup()` targeting a **module-level function** (e.g., `dispatch._runcommand`, `wireproto._capabilities`) — this runs per-repository, so the function gets wrapped multiple times (once per repo opened in the session), causing a wrap-stack that grows with each repo access
- `reposetup()` that does expensive one-time initialization without guarding against repeated calls
- Accessing `repo` in `uisetup()` or `extsetup()` — no repository is available yet

## Do NOT Flag

- `extensions.wrapfunction()` in `extsetup()` — this is the correct place, all extensions are loaded, runs once per session
- `extensions.wrapcommand()` in `extsetup()` — correct, command table is fully populated
- `extensions.wrapfunction()` in `uisetup()` targeting **core sapling modules** (e.g., `sapling.dispatch`, `sapling.exchange`, `sapling.hg`, `sapling.localrepo`, `sapling.commands`, `sapling.util`, `sapling.i18n`) — these are always available and this is an established pattern used by `treemanifest`, `amend`, `dialect`, `blackbox`, `clienttelemetry`, and many others
- `extensions.afterloaded("extname", callback)` in `uisetup()` — this is the correct pattern for wrapping other extensions that may not be loaded yet
- `reposetup()` that only sets per-repo config or registers per-repo hooks — these are meant to run per-repo
- `extensions.wrapfunction()` in `reposetup()` targeting **per-repo object methods** (e.g., `repo.dirstate`, class methods on repo-scoped objects) — each repo gets its own wrap, which is intentional
- `uisetup()` that modifies `ui` defaults or registers non-extension-dependent callbacks
- `cmdtable` and `command = registrar.command(cmdtable)` at module level — inert registration

## Examples

**BAD (wrapfunction in uisetup targeting another extension — may not be loaded):**
```python
def uisetup(ui):
    # WRONG: commitcloud extension may not be loaded yet
    extensions.wrapfunction(
        commitcloud, "sync", _mysyncwrapper
    )
```

**GOOD (use afterloaded for extension targets in uisetup):**
```python
def uisetup(ui):
    def _wrapper(loaded):
        if loaded:
            ext = extensions.find("commitcloud")
            extensions.wrapfunction(ext, "sync", _mysyncwrapper)
    extensions.afterloaded("commitcloud", _wrapper)
```

**GOOD (wrapfunction in uisetup targeting core modules — always available):**
```python
def uisetup(ui):
    # Core sapling modules are always loaded, this is safe and common
    extensions.wrapfunction(exchange, "push", _mypushwrapper)
```

**GOOD (wrapfunction in extsetup — all extensions loaded):**
```python
def extsetup(ui):
    extensions.wrapfunction(
        commitcloud, "sync", _mysyncwrapper
    )
```

**BAD (wrapfunction in reposetup — wraps multiple times):**
```python
def reposetup(ui, repo):
    # WRONG: runs for every repo opened in the session
    # After opening 3 repos, dispatch._runcommand is wrapped 3 times
    extensions.wrapfunction(dispatch, "_runcommand", _mywrapper)
```

**GOOD (one-time setup in extsetup, per-repo config in reposetup):**
```python
def extsetup(ui):
    extensions.wrapfunction(dispatch, "_runcommand", _mywrapper)

def reposetup(ui, repo):
    # per-repo config is fine here
    if repo.ui.configbool("myext", "enabled"):
        repo.ui.setconfig("hooks", "pre-commit.myext", _myhook)
```
