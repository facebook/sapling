---
sidebar_position: 44
---

## undo
<!--
  @generated SignedSource<<3cc2cbc7f0eb98479a55c103e2495eb4>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**undo the last local command**

Reverse the effects of the last local command. A local command is one that
changed the currently checked out commit, that modified the contents of
local commits, or that changed local bookmarks. Examples of local commands
include `sl goto`, `sl commit`, `sl amend`, and `sl rebase`.

You cannot use `sl undo` to undo uncommited changes in the working copy,
or changes to remote bookmarks.

You can run `sl undo` multiple times to undo a series of local commands.
Alternatively, you can explicitly specify the number of local commands to
undo using `--step`. This number can also be specified as a positional
argument.

To undo the effects of `sl undo`, run `sl redo`. Run
`sl help redo` for more information.

Include `--keep` to preserve the state of the working copy. For example,
specify `--keep` when running `sl undo` to reverse the effects of an
`sl commit` or `sl amend` operation while still preserving changes
in the working copy. These changes will appear as pending changes.

Specify `--preview` to see a graphical display that shows what
your smartlog will look like after you run the command. Specify
`--interactive` for an interactive version of this preview in which
you can step backwards and forwards in the undo history.

`sl undo` cannot be used with non-local commands, or with commands
that are read-only. `sl undo` will skip over these commands in the
undo history.

For hybrid commands that result in both local and remote changes,
`sl undo` will undo the local changes, but not the remote changes.
For example, `@prog pull --rebase` might move remote/master and also
rebase local commits. In this situation, `sl undo` will revert the
rebase, but not the change to remote/master.

Branch limits the scope of an undo to a group of local (draft)
changectxs, identified by any one member of this group.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-a`| `--absolute`| `false`| absolute based on command index instead of relative undo|
| `-i`| `--interactive`| `false`| use interactive ui for undo|
| `-k`| `--keep`| `false`| keep working copy changes|
| `-n`| `--step`| `1`| how many steps to undo back|
| `-p`| `--preview`| `false`| see smartlog-like preview of future undo state|
