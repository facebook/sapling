---
sidebar_position: 3
---

## amend | am
<!--
  @generated SignedSource<<a67553916fa9b0f9dcf8d863fb1ecfe5>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**meld pending changes into the current commit**

Replace your current commit with a new commit that contains the contents
of the original commit, plus any pending changes.

By default, all pending changes (in other words, those reported by
`sl status`) are committed. To commit only some of your
changes, you can:

- Specify an exact list of files for which you want changes committed.

- Use the `-I` or `-X` flags to match file names to exclude or  include using patterns or filesets. See `sl help patterns` and `sl help filesets`.

- Specify the `--interactive` flag to open a UI where you can  select individual hunks for inclusion.

By default, `sl amend` reuses your existing commit message and does not
prompt you for changes. To change your commit message, you can:

- Specify `--edit/-e` to open your configured editor to update the  existing commit message.

- Specify `--message/-m` to replace the entire commit message, including  any commit template fields, with a string that you specify.

Specifying `-m` overwrites all information in the commit message,
including information specified as part of a pre-loaded commit
template. For example, any information associating this commit with
a code review system will be lost and might result in breakages.

When you amend a commit that has descendants, those descendants are
rebased on top of the amended version of the commit, unless doing so
would result in merge conflicts. If this happens, run `sl restack`
to manually trigger the rebase so that you can go through the merge
conflict resolution process. Alternatively:

- Specify `--rebase` to always trigger the rebase and resolve merge  conflicts.

- Specify `--no-rebase` to prevent the automatic rebasing of descendants.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-A`| `--addremove`| | mark new/missing files as added/removed before committing|
| `-e`| `--edit`| | prompt to edit the commit message|
| `-i`| `--interactive`| | use interactive mode|
| | `--rebase`| | rebases children after the amend|
| `-T`| `--template`| | display with template|
| `-I`| `--include`| | include files matching the given patterns|
| `-X`| `--exclude`| | exclude files matching the given patterns|
| `-m`| `--message`| | use text as commit message|
| `-l`| `--logfile`| | read commit message from file|
| `-d`| `--date`| | record the specified date as commit date|
| `-u`| `--user`| | record the specified user as committer|
| | `--no-move-detection`| | disable automatic file move detection|
| | `--stack`| | incorporate corrections into stack. see 'sl help absorb' for details|
