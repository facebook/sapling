---
sidebar_position: 21
---

## log | history
<!--
  @generated SignedSource<<a0cb2310265f830f31e04fddb67321fc>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**show commit history**

Print the revision history of the specified files or the entire
project.

If no revision range is specified, the default is `::.`.

File history is shown without following rename or copy history of
files. Use -f/--follow with a filename to follow history across
renames and copies. --follow without a filename will only show
ancestors or descendants of the starting revision.

By default this command prints revision number and changeset id,
non-trivial parents, user, date and time, and a summary for
each commit. When the -v/--verbose switch is used, the list of
changed files and full commit message are shown.

With --graph the revisions are shown as an ASCII art DAG with the most
recent changeset at the top.
'o' is a changeset, '@' is a working directory parent, 'x' is obsolete,
and '+' represents a fork where the changeset from the lines below is a
parent of the 'o' merge on the same line.
Paths in the DAG are represented with '|', '/' and so forth. ':' in place
of a '|' indicates one or more revisions in a path are omitted.

Use -L/--line-range FILE,M:N options to follow the history of lines
from M to N in FILE. With -p/--patch only diff hunks affecting
specified line range will be shown. This option requires --follow;
it can be specified multiple times. Currently, this option is not
compatible with --graph. This option is experimental.

`sl log --patch` may generate unexpected diff output for merge
changesets, as it will only compare the merge changeset against
its first parent. Also, only files different from BOTH parents
will appear in files:.

For performance reasons, `sl log FILE` may omit duplicate changes
made on branches and will not show removals or mode changes. To
see all such changes, use the --removed switch.

The history resulting from -L/--line-range options depends on diff
options; for instance if white-spaces are ignored, respective changes
with only white-spaces in specified line range will not be listed.

Some examples:

- changesets with full descriptions and file lists:

```
sl log -v
```

- changesets ancestral to the working directory:

```
sl log -f
```

- last 10 commits on the current branch:

```
sl log -l 10 -b .
```

- changesets showing all modifications of a file, including removals:

```
sl log --removed file.c
```

- all changesets that touch a directory, with diffs, excluding merges:

```
sl log -Mp lib/
```

- all revision numbers that match a keyword:

```
sl log -k bug --template "{rev}\n"
```

- the full hash identifier of the working directory parent:

```
sl log -r . --template "{node}\n"
```

- list available log templates:

```
sl log -T list
```

- check if a given changeset is included in a bookmarked release:

```
sl log -r "a21ccf and ancestor(release_1.9)"
```

- find all changesets by some user in a date range:

```
sl log -k alice -d "may 2008 to jul 2008"
```

- changesets touching lines 13 to 23 for file.c:

```
sl log -L file.c,13:23
```

- changesets touching lines 13 to 23 for file.c and lines 2 to 6 of  main.c with patch:

```
sl log -L file.c,13:23 -L main.c,2:6 -p
```

See `sl help dates` for a list of formats valid for -d/--date.

See `sl help revisions` for more about specifying and ordering
revisions.

See `sl help templates` for more about pre-packaged styles and
specifying custom templates. The default template used by the log
command can be customized via the `ui.logtemplate` configuration
setting.

Returns 0 on success.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-f`| `--follow`| | follow changeset history, or file history across copies and renames|
| `-d`| `--date`| | show revisions matching date spec|
| `-C`| `--copies`| | show copied files|
| `-k`| `--keyword`| | do case-insensitive search for a given text|
| `-r`| `--rev`| | show the specified revision or revset|
| | `--removed`| | include revisions where files were removed|
| `-u`| `--user`| | revisions committed by user|
| `-b`| `--branch`| | show changesets within the given named branch|
| `-P`| `--prune`| | do not display revision or any of its ancestors|
| `-p`| `--patch`| | show patch|
| `-g`| `--git`| | use git extended diff format|
| `-l`| `--limit`| | limit number of changes displayed|
| `-M`| `--no-merges`| | do not show merges|
| | `--stat`| | output diffstat-style summary of changes|
| `-G`| `--graph`| | show the revision DAG|
| `-T`| `--template`| | display with template|
| `-I`| `--include`| | include files matching the given patterns|
| `-X`| `--exclude`| | exclude files matching the given patterns|
| | `--all`| | shows all changesets in the repo|
| | `--sparse`| | limit to changesets affecting the sparse checkout|
| | `--remote`| | show remote names even if hidden|
