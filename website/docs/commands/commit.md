---
sidebar_position: 9
---

## commit | ci
<!--
  @generated SignedSource<<63c5d5ad2a3b0561efa3a15bdfedc890>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**save all pending changes or specified files in a new commit**

Commit changes to the given files to your local repository.

By default, all pending changes (in other words, those reported by
`sl status`) are committed. If you want to commit only some of your
changes, choose one of the following options:

- Specify an exact list of files for which you want changes committed.

- Use the `-I` or `-X` flags to match or exclude file names  using a pattern or fileset. See `sl help patterns` and  `sl help filesets` fot details.

- Specify the `--interactive` flag to open a UI to select  individual files, hunks, or lines.

To meld pending changes into the current commit instead of creating
a new commit, see `sl amend`.

If you are committing the result of a merge, such as when merge
conflicts occur during `sl checkout`, commit all pending changes.
Do not specify files or use `-I`, `-X`, or `-i`.

Specify the `-m` flag to include a free-form commit message. If you do
not specify `-m`, Sapling opens your configured editor where you can
enter a message based on a pre-loaded commit template.

Returns 0 on success, 1 if nothing changed.

If your commit fails, you can find a backup of your commit message in
`.sl/last-message.txt`.

Examples:

- commit all files ending in .py:

```
sl commit --include "glob:**.py"
```

- commit all non-binary files:

```
sl commit --exclude "set:binary()"
```

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-A`| `--addremove`| | mark new/missing files as added/removed before committing|
| `-e`| `--edit`| | invoke editor on commit messages|
| `-i`| `--interactive`| | use interactive mode|
| `-M`| `--reuse-message`| | reuse commit message from REV|
| `-I`| `--include`| | include files matching the given patterns|
| `-X`| `--exclude`| | exclude files matching the given patterns|
| `-m`| `--message`| | use text as commit message|
| `-l`| `--logfile`| | read commit message from file|
| `-d`| `--date`| | record the specified date as commit date|
| `-u`| `--user`| | record the specified user as committer|
| | `--no-move-detection`| | disable automatic file move detection|
