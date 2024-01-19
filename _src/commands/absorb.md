---
sidebar_position: 0
---

## absorb | ab
<!--
  @generated SignedSource<<34fa88c23169ca5927cb1077afb89311>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**intelligently integrate pending changes into current stack**

Attempt to amend each pending change to the proper commit in your
stack. Absorb does not write to the working copy.

If absorb cannot find an unambiguous commit to amend for a change, that
change will be left in the working copy, untouched. The unabsorbed
changes can be observed by `sl status` or `sl diff` afterwards.

Commits outside the revset `::. and not public() and not merge()` will
not be changed.

Commits that become empty after applying the changes will be deleted.

By default, absorb will show what it plans to do and prompt for
confirmation.  If you are confident that the changes will be absorbed
to the correct place, run `sl absorb -a` to apply the changes
immediately.

Returns 0 if anything was absorbed, 1 if nothing was absorbed.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-a`| `--apply-changes`| | apply changes without prompting for confirmation|
| `-d`| `--date`| | record the specified date as commit date|
| `-n`| `--dry-run`| | do not perform actions, just print output|
| `-T`| `--template`| | display with template|
| `-I`| `--include`| | include files matching the given patterns|
| `-X`| `--exclude`| | exclude files matching the given patterns|
