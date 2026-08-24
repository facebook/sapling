---
name: ACR-demand-import-violation
metadata:
  oncalls: ['scm_client_infra']
  strict: true
  apply_to_path: 'eden/scm/sapling/(commands/|ext/).*\.py$'
  apply_to_content: 'hgdemandimport|cmdtable'
  apply_to_clients: ['code_review']
---

# Demand Import Violation

**Severity: HIGH**

## What to Look For

- Top-level imports in command modules (`sapling/commands/`) and extensions (`sapling/ext/`) that have side effects on the command table or global state
- Imports that must be inside `with hgdemandimport.deactivated():` blocks but aren't
- Extension modules that import other sapling modules at the top level when those modules register commands

## When to Flag

- A new command module in `sapling/commands/` that is imported at the top of `commands/__init__.py` outside the `with hgdemandimport.deactivated():` block — the import has side effects (populates `cmdtable`) that demand-import would defer incorrectly
- Extension code that does `from sapling import commands` or similar imports that trigger command registration as a side effect

## Do NOT Flag

- Standard library imports at the top level (`import os`, `import re`, etc.)
- Top-level `import bindings` — this is the established pattern used by 75+ files in the codebase (`util.py`, `smartset.py`, `dispatch.py`, `commands/__init__.py`, etc.)
- Imports inside functions or methods — these are already lazy
- Imports inside `with hgdemandimport.deactivated():` blocks — this is the correct pattern
- `from .cmdtable import command` in command modules — this is the standard registration pattern
- Extension modules that only define `cmdtable = {}` and `command = registrar.command(cmdtable)` at module level — these are inert until the extension is loaded

## Examples

**BAD (command import outside deactivated block):**
```python
# In sapling/commands/__init__.py
from . import mynewcommand  # WRONG: side effects deferred by demand-import
# The command table won't be populated until something actually accesses
# an attribute of mynewcommand, which may never happen

with hgdemandimport.deactivated():
    from . import commit, log, status  # existing correct imports
```

**GOOD (inside deactivated block):**
```python
with hgdemandimport.deactivated():
    from . import commit, log, status, mynewcommand
```
