---
sidebar_position: 30
---

## rebase
<!--
  @generated SignedSource<<ff1197d75ec90e60bab3409eeb0c4442>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**move commits from one location to another**

Move commits from one part of the commit graph to another. This
behavior is achieved by creating a copy of the commit at the
destination and hiding the original commit.

Use `-k/--keep` to skip the hiding and keep the original commits visible.

If the commits being rebased have bookmarks, rebase moves the bookmarks
onto the new versions of the commits. Bookmarks are moved even if `--keep`
is specified.

Public commits cannot be rebased unless you use the `--keep` option
to copy them.

Use the following options to select the commits you want to rebase:

1. `-r/--rev` to explicitly select commits

2. `-s/--source` to select a root commit and include all of its   descendants

3. `-b/--base` to select a commit and its ancestors and descendants

If no option is specified to select commits, `-b .` is used by default.

If `--source` or `--rev` is used, special names `SRC` and `ALLSRC`
can be used in `--dest`. Destination would be calculated per source
revision with `SRC` substituted by that single source revision and
`ALLSRC` substituted by all source revisions.

If commits that you are rebasing consist entirely of changes that are
already present in the destination, those commits are not moved (in
other words, they are rebased out).

Sometimes conflicts can occur when you rebase. When this happens, by
default, Sapling launches an editor for every conflict. Conflict markers
are inserted into affected files, like:

```
<<<<
dest
====
source
>>>>
```

To fix the conflicts, for each file, remove the markers and replace the
whole block of code with the correctly merged code.

If you close the editor without resolving the conflict, the rebase is
interrupted and you are returned to the command line. At this point, you
can resolve conflicts in manual resolution mode. See `sl help resolve` for
details.

After manually resolving conflicts, resume the rebase with
`sl rebase --continue`. If you are not able to successfully
resolve all conflicts, run `sl rebase --abort` to abort the
rebase.

Alternatively, you can use a custom merge tool to automate conflict
resolution. To specify a custom merge tool, use the `--tool` flag. See
`sl help merge-tools` for a list of available tools and for information
about configuring the default merge behavior.

Examples:

- Move a single commit to master:

```
sl rebase -r 5f493448 -d master
```

- Move a commit and all its descendants to another part of the commit graph:

```
sl rebase --source c0c3 --dest 4cf9
```

- Rebase everything on a local branch marked by a bookmark to master:

```
sl rebase --base myfeature --dest master
```

- Rebase orphaned commits onto the latest version of their parents:

```
sl rebase --restack
```

Configuration Options:

You can make rebase require a destination if you set the following config
option:

```
[commands]
rebase.requiredest = True
```

By default, rebase will close the transaction after each commit. For
performance purposes, you can configure rebase to use a single transaction
across the entire rebase. WARNING: This setting introduces a significant
risk of losing the work you've done in a rebase if the rebase aborts
unexpectedly:

```
[rebase]
singletransaction = True
```

By default, rebase writes to the working copy, but you can configure it
to run in-memory for for better performance, and to allow it to run if the
current checkout is dirty:

```
[rebase]
experimental.inmemory = True
```

It will also print a configurable warning:

```
[rebase]
experimental.inmemorywarning = Using experimental in-memory rebase
```

Returns 0 on success (also when nothing to rebase), 1 if there are
unresolved conflicts.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-s`| `--source`| | rebase the specified commit and descendants|
| `-b`| `--base`| | rebase everything from branching point of specified commit|
| `-r`| `--rev`| | rebase these revisions|
| `-d`| `--dest`| | rebase onto the specified revision|
| | `--collapse`| `false`| collapse the rebased commits|
| `-m`| `--message`| | use text as collapse commit message|
| `-e`| `--edit`| `false`| invoke editor on commit messages|
| `-l`| `--logfile`| | read collapse commit message from file|
| `-k`| `--keep`| `false`| keep original commits|
| `-t`| `--tool`| | specify merge tool|
| `-c`| `--continue`| `false`| continue an interrupted rebase|
| `-a`| `--abort`| `false`| abort an interrupted rebase|
| | `--restack`| `false`| rebase all changesets in the current stack onto the latest version of their respective parents|
| `-i`| `--interactive`| `false`| interactive rebase|
