---
sidebar_position: 6
---

## bookmark | bo | book
<!--
  @generated SignedSource<<81689317a83457c326f404c55f3e4de0>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**create a new bookmark or list existing bookmarks**

Bookmarks are labels on changesets to help track lines of development.
Bookmarks are unversioned and can be moved, renamed and deleted.
Deleting or moving a bookmark has no effect on the associated changesets.

Creating or updating to a bookmark causes it to be marked as 'active'.
The active bookmark is indicated with a '*'.
When a commit is made, the active bookmark will advance to the new commit.
A plain `sl update` will also advance an active bookmark, if possible.
Updating away from a bookmark will cause it to be deactivated.

Bookmarks can be pushed and pulled between repositories (see
`sl help push` and `sl help pull`). If a shared bookmark has
diverged, a new 'divergent bookmark' of the form 'name@path' will
be created. Using `sl merge` will resolve the divergence.

Specifying bookmark as '.' to -m or -d options is equivalent to specifying
the active bookmark's name.

A bookmark named '@' has the special property that `sl clone` will
check it out by default if it exists.

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
| | `--list-subscriptions`| | show only remote bookmarks that are available locally|
