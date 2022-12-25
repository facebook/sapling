---
sidebar_position: 6
---

## bookmark | bo | book
<!--
  @generated SignedSource<<33d9c3a4e740d1830162333f66ce45e5>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**create a new bookmark or list existing bookmarks**

Bookmarks are labels on changesets to help track lines of development.
Bookmarks are unversioned and can be moved, renamed and deleted.
Deleting or moving a bookmark has no effect on the associated changesets.

Creating or updating to a bookmark causes it to be marked as 'active'.
The active bookmark is indicated with a '*'.
When a commit is made, the active bookmark will advance to the new commit.
A plain `sl goto` will also advance an active bookmark, if possible.
Updating away from a bookmark will cause it to be deactivated.

Bookmarks can be pushed and pulled between repositories (see
`sl help push` and `sl help pull`). If a shared bookmark has
diverged, a new 'divergent bookmark' of the form 'name@path' will
be created. Using `sl merge` will resolve the divergence.

Specifying bookmark as '.' to -m or -d options is equivalent to specifying
the active bookmark's name.

Examples:

- create an active bookmark for a new line of development:

```
sl book new-feature
```

- create an inactive bookmark as a place marker:

```
sl book -i reviewed
```

- create an inactive bookmark on another changeset:

```
sl book -r .^ tested
```

- rename bookmark turkey to dinner:

```
sl book -m turkey dinner
```

- move the '@' bookmark from another branch:

```
sl book -f @
```

In Git repos, bookmarks correspond to branches. Remote Git branches can be listed using the `--remote` flag.

Examples:

- list remote branches:

```
sl bookmark --remote
```

- list remote tags:

```
sl bookmark --remote tags
```

- list all refs:

```
sl bookmark --remote 'refs/*'
```

- list branches from specified path:

```
sl bookmark --remote --remote-path my-fork
```

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-f`| `--force`| `false`| force|
| `-r`| `--rev`| | revision for bookmark action|
| `-d`| `--delete`| `false`| delete a given bookmark|
| `-D`| `--strip`| | like --delete, but also strip changesets|
| `-m`| `--rename`| | rename a given bookmark|
| `-i`| `--inactive`| `false`| mark a bookmark inactive|
| `-t`| `--track`| | track this bookmark or remote name|
| `-u`| `--untrack`| | remove tracking for this bookmark|
| `-a`| `--all`| | show both remote and local bookmarks|
| | `--remote`| | fetch remote Git refs|
| | `--remote-path`| | remote path from which to fetch bookmarks|
| | `--list-subscriptions`| | show only remote bookmarks that are available locally|
