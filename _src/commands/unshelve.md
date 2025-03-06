---
sidebar_position: 46
---

## unshelve
<!--
  @generated SignedSource<<eb96d08a45c3c2969b324dc9160eb5fe>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**restore a shelved change to the working copy**

This command accepts an optional name of a shelved change to
restore. If none is given, the most recent shelved change is used.

If a shelved change is applied successfully, the bundle that
contains the shelved changes is moved to a backup location
(.sl/shelve-backup).

Since you can restore a shelved change on top of an arbitrary
commit, it is possible that unshelving will result in a conflict. If
this occurs, you must resolve the conflict, then use `--continue`
to complete the unshelve operation. The bundle will not be moved
until you successfully complete the unshelve.

Alternatively, you can use `--abort` to cancel the conflict
resolution and undo the unshelve, leaving the shelve bundle intact.

After a successful unshelve, the shelved changes are stored in a
backup directory. Only the N most recent backups are kept. N
defaults to 10 but can be overridden using the `shelve.maxbackups`
configuration option.

Timestamp in seconds is used to decide the order of backups. More
than `maxbackups` backups are kept if same timestamp prevents
from deciding exact order of them, for safety.

Returns 0 on success.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-a`| `--abort`| | abort an incomplete unshelve operation|
| `-c`| `--continue`| | continue an incomplete unshelve operation|
| `-k`| `--keep`| | keep shelve after unshelving|
| `-n`| `--name`| | restore shelved change with given name|
| `-t`| `--tool`| | specify merge tool|
