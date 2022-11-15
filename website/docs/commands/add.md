---
sidebar_position: 1
---

## add
<!--
  @generated SignedSource<<ff8b04e2ceb360d930b9c3af0d136a8b>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**start tracking the specified files**

Specify files to be tracked by Sapling. The files will be added to
the repository at the next commit.

To undo an add before files have been committed, use `sl forget`.
To undo an add after files have been committed, use `sl rm`.

If no names are given, add all files to the repository (except
files matching `.gitignore`).

Examples:

- New (unknown) files are added  automatically by `sl add`:

```
$ ls
foo.c
$ sl status
? foo.c
$ sl add
adding foo.c
$ sl status
A foo.c
```

- Add specific files:

```
$ ls
bar.c  foo.c
$ sl status
? bar.c
? foo.c
$ sl add bar.c
$ sl status
A bar.c
? foo.c
```

Returns 0 if all files are successfully added.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-I`| `--include`| | include files matching the given patterns|
| `-X`| `--exclude`| | exclude files matching the given patterns|
| `-n`| `--dry-run`| | do not perform actions, just print output|
| `-s`| `--sparse`| | also include directories of added files in sparse config|
