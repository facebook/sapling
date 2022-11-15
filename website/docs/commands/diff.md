---
sidebar_position: 10
---

## diff | d
<!--
  @generated SignedSource<<28e0a87ecefceb3c9cef0593e30b314e>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**show differences between commits**

Show the differences between two commits. If only one commit is specified,
show the differences between the specified commit and your working copy.
If no commits are specified, show your pending changes.

Specify `-c` to see the changes in the specified commit relative to its
parent.

By default, this command skips binary files. To override this behavior,
specify `-a` to include binary files in the diff.

By default, diffs are shown using the unified diff format. Specify `-g`
to generate diffs in the git extended diff format. For more information,
see `sl help diffs`.

`sl diff` might generate unexpected results during merges because it
defaults to comparing against your working copy's first parent commit
if no commits are specified.

Examples:

- compare a file in the current working directory to its parent:

```
sl diff foo.c
```

- compare two historical versions of a directory, with rename info:

```
sl diff --git -r 5be761874:431ec8e07 lib/
```

- get change stats relative to the last change on some date:

```
sl diff --stat -r "date('may 2')"
```

- diff all newly-added files that contain a keyword:

```
sl diff "set:added() and grep(GNU)"
```

- compare a revision and its parents:

```
sl diff -c 340f3fef5              # compare against first parent
sl diff -r 340f3fef5^:340f3fef5   # same using revset syntax
sl diff -r 340f3fef5^2:340f3fef5  # compare against the second parent
```

Returns 0 on success.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-r`| `--rev`| | revision|
| `-c`| `--change`| | change made by revision|
| `-a`| `--text`| | treat all files as text|
| `-g`| `--git`| | use git extended diff format|
| | `--binary`| | generate binary diffs in git mode (default)|
| | `--nodates`| | omit dates from diff headers|
| | `--noprefix`| | omit a/ and b/ prefixes from filenames|
| `-p`| `--show-function`| | show which function each change is in|
| | `--reverse`| | produce a diff that undoes the changes|
| `-w`| `--ignore-all-space`| | ignore white space when comparing lines|
| `-b`| `--ignore-space-change`| | ignore changes in the amount of white space|
| `-B`| `--ignore-blank-lines`| | ignore changes whose lines are all blank|
| `-Z`| `--ignore-space-at-eol`| | ignore changes in whitespace at EOL|
| `-U`| `--unified`| | number of lines of context to show|
| | `--stat`| | output diffstat-style summary of changes|
| | `--root`| | produce diffs relative to subdirectory|
| | `--only-files-in-revs`| | only show changes for files modified in the requested revisions|
| `-I`| `--include`| | include files matching the given patterns|
| `-X`| `--exclude`| | exclude files matching the given patterns|
| `-s`| `--sparse`| | only show changes in files in the sparse config|
