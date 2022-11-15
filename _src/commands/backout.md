---
sidebar_position: 5
---

## backout
<!--
  @generated SignedSource<<9afd5051d11c6d14fd3334dc8c149510>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**reverse the effects of an earlier commit**

Create an inverse commit of the specified commit. Backout is commonly
used to undo the effects of a public commit.

By default, `sl backout` creates a new commit on top of the
current commit. Specify `--no-commit` to skip making a new
commit, leaving the changes outstanding in your working copy.

If merge conflicts are encountered during the backout, changes will be
left in the working copy with conflict markers inserted. When this occurs,
resolve the conflicts and then run `sl commit`.

By default, `sl backout` will abort if pending changes are present in the
working copy. Specify `--merge` to combine changes from the backout with
your pending changes.

Examples:

- Reverse the effect of the parent of the working copy.  This backout will be committed immediately:

```
sl backout -r .
```

- Reverse the effect of previous bad commit 42e8ddebe:

```
sl backout -r 42e8ddebe
```

- Reverse the effect of previous bad revision 42e8ddebe and  leave changes uncommitted:

```
sl backout -r 42e8ddebe --no-commit
sl commit -m "Backout 42e8ddebe"
```

By default, the new commit will have one parent,
maintaining a linear history. With `--merge`, the commit
will instead have two parents: the old parent of the
working copy and a new child of REV that simply undoes REV.

See `sl help dates` for a list of formats valid for `-d/--date`.

See `sl help revert` for a way to restore files to the state
of another revision.

Returns 0 on success, 1 if nothing to backout or there are unresolved
files.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| | `--merge`| | combine existing pending changes with backout changes|
| | `--no-commit`| `false`| do not commit|
| `-r`| `--rev`| | revision to back out|
| `-e`| `--edit`| `false`| open editor to specify custom commit message|
| `-t`| `--tool`| | specify merge tool|
| `-I`| `--include`| | include files matching the given patterns|
| `-X`| `--exclude`| | exclude files matching the given patterns|
| `-m`| `--message`| | use text as commit message|
| `-l`| `--logfile`| | read commit message from file|
| `-d`| `--date`| | record the specified date as commit date|
| `-u`| `--user`| | record the specified user as committer|
