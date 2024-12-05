---
sidebar_position: 41
---

## status | st
<!--
  @generated SignedSource<<a3c52cd22ade381d4bfcb2b663c552d6>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**list files with pending changes**

Show status of files in the working copy using the following status
indicators:

```
M = modified
A = added
R = removed
C = clean
! = missing (deleted by a non-sl command, but still tracked)
? = not tracked
I = ignored
  = origin of the previous file (with --copies)
```

By default, shows files that have been modified, added, removed,
deleted, or that are unknown (corresponding to the options `-mardu`,
respectively). Files that are unmodified, ignored, or the source of
a copy/move operation are not listed.

To control the exact statuses that are shown, specify the relevant
flags (like `-rd` to show only files that are removed or deleted).
Additionally, specify `-q/--quiet` to hide both unknown and ignored
files.

To show the status of specific files, provide a list of files to
match. To include or exclude files using patterns or filesets, use
`-I` or `-X`.

If `--rev` is specified and only one revision is given, it is used as
the base revision. If two revisions are given, the differences between
them are shown. The `--change` option can also be used as a shortcut
to list the changed files of a revision from its first parent.

`sl status` might appear to disagree with `sl diff` if permissions
have changed or a merge has occurred, because the standard diff
format does not report permission changes and `sl diff` only
reports changes relative to one merge parent.

The `-t/--terse` option abbreviates the output by showing only the directory
name if all the files in it share the same status. The option takes an
argument indicating the statuses to abbreviate: &#x27;m&#x27; for &#x27;modified&#x27;, &#x27;a&#x27;
for &#x27;added&#x27;, &#x27;r&#x27; for &#x27;removed&#x27;, &#x27;d&#x27; for &#x27;deleted&#x27;, &#x27;u&#x27; for &#x27;unknown&#x27;, &#x27;i&#x27;
for &#x27;ignored&#x27; and &#x27;c&#x27; for clean.

It abbreviates only those statuses which are passed. Note that clean and
ignored files are not displayed with `--terse ic` unless the `-c/--clean`
and `-i/--ignored` options are also used.

The `-v/--verbose` option shows information when the repository is in an
unfinished merge, shelve, rebase state, etc. You can have this behavior
turned on by default by enabling the `commands.status.verbose` config option.

You can skip displaying some of these states by setting
`commands.status.skipstates` to one or more of: &#x27;bisect&#x27;, &#x27;graft&#x27;,
&#x27;histedit&#x27;, &#x27;merge&#x27;, &#x27;rebase&#x27;, or &#x27;unshelve&#x27;.

Examples:

- show changes in the working directory relative to a  commit:

```
sl status --rev 88a692db8
```

- show changes in the working copy relative to the  current directory (see `sl help patterns` for more information):

```
sl status re:
```

- show all changes including copies in a commit:

```
sl status --copies --change 88a692db8
```

- get a NUL separated list of added files, suitable for xargs:

```
sl status -an0
```

- show more information about the repository status, abbreviating  added, removed, modified, deleted, and untracked paths:

```
sl status -v -t mardu
```

Returns 0 on success.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-A`| `--all`| `false`| show status of all files|
| `-m`| `--modified`| `false`| show only modified files|
| `-a`| `--added`| `false`| show only added files|
| `-r`| `--removed`| `false`| show only removed files|
| `-d`| `--deleted`| `false`| show only deleted (but tracked) files|
| `-c`| `--clean`| `false`| show only files without changes|
| `-u`| `--unknown`| `false`| show only unknown (not tracked) files|
| `-i`| `--ignored`| `false`| show only ignored files|
| `-n`| `--no-status`| `false`| hide status prefix|
| `-C`| `--copies`| `false`| show source of copied files|
| `-0`| `--print0`| `false`| end filenames with NUL, for use with xargs|
| | `--rev`| | show difference from revision|
| | `--change`| | list the changed files of a revision|
| | `--root-relative`| | show status relative to root|
| `-I`| `--include`| | include files matching the given patterns|
| `-X`| `--exclude`| | exclude files matching the given patterns|
