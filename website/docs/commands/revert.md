---
sidebar_position: 34
---

## revert
<!--
  @generated SignedSource<<9c8a87a854f9e334299c540a7586eff8>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**change the specified files to match a commit**

With no revision specified, restore the contents of files to an
unmodified state and unschedule adds, removes, copies, and renames.
In other words, revert the specified files or directories to the
contents they had in the current commit. If you are in the middle of
an unfinished merge state, you must explicitly specify a revision.

Use the `-r/--rev` option to revert the given files or directories to
their states as of a specific commit. Because revert does not actually
check out the specified commit, the files appear as modified and show
up as pending changes in `sl status`.

Revert causes files to match their contents in another commit. If
instead you want to undo a specific landed commit, use `sl backout`
instead. Run `sl help backout` for more information.

Modified files are saved with an .orig suffix before reverting.
To disable these backups, use `--no-backup`. You can configure Sapling
to store these backup files in a custom directory relative to the root
of the repository by setting the `ui.origbackuppath` configuration
option.

Returns 0 on success.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-a`| `--all`| | revert all changes when no arguments given|
| `-d`| `--date`| | tipmost revision matching date|
| `-r`| `--rev`| | revert to the specified revision|
| `-C`| `--no-backup`| | do not save backup copies of files|
| `-i`| `--interactive`| | interactively select the changes|
| `-I`| `--include`| | include files matching the given patterns|
| `-X`| `--exclude`| | exclude files matching the given patterns|
| `-n`| `--dry-run`| | do not perform actions, just print output|
