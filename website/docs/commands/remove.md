---
sidebar_position: 32
---

## remove | rm
<!--
  @generated SignedSource<<8b7776d140c433587dd3376798892d5d>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**delete the specified tracked files**

Remove the specified tracked files from the repository and delete
them. The files will be deleted from the repository at the next
commit.

To undo a remove before files have been committed, use `sl revert`.
To stop tracking files without deleting them, use `sl forget`.

`-A/--after` can be used to remove only files that have already
been deleted, `-f/--force` can be used to force deletion, and `-Af`
can be used to remove files from the next revision without
deleting them from the working directory.

The following table details the behavior of remove for different
file states (columns) and option combinations (rows). The file
states are Added (**A**), Clean (**C**), Modified (**M**) and
Missing (**!**) (as reported by `sl status`). The actions are
Warn (**W**), Remove (**R**) (from branch) and Delete (**D**)
(from disk):

| | | | | |
| - | - | - | - | - |
| opt/state | **A** | **C** | **M** | **!** |
| none | **W** | **RD** | **W** | **R** |
| ``-f`` | **R** | **RD** | **RD** | **R** |
| ``-A`` | **W** | **W** | **W** | **R** |
| ``-Af`` | **R** | **R** | **R** | **R** |

`sl remove` never deletes files in **Added** state from the
working directory, not even if `--force` is specified.

Returns 0 on success, 1 if any warnings encountered.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-A`| `--after`| | record delete for missing files|
| `-f`| `--force`| | forget added files, delete modified files|
| `-I`| `--include`| | include files matching the given patterns|
| `-X`| `--exclude`| | exclude files matching the given patterns|
