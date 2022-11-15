---
sidebar_position: 29
---

## push
<!--
  @generated SignedSource<<02e4c69c70a2a2ad55794f262d7cb298>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**push changes to the specified destination**

Push commits from the local repository to the specified
destination.

By default, push does not allow creation of new heads at the
destination since multiple heads make it unclear which head
to use. In this situation, it is recommended to pull and merge
before pushing.

Extra care should be taken with the `-f/--force` option,
which will push all new heads on all branches, an action which will
almost always cause confusion for collaborators.

If `-r/--rev` is used, the specified revision and all its ancestors
will be pushed to the remote repository.

If `-B/--bookmark` is used, the specified bookmarked revision, its
ancestors, and the bookmark will be pushed to the remote
repository. Specifying `.` is equivalent to specifying the active
bookmark's name.

Please see `sl help urls` for important details about `ssh://`
URLs. If DESTINATION is omitted, a default path will be used.

Returns 0 if push was successful, 1 if nothing to push.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-f`| `--force`| | force push|
| `-r`| `--rev`| | a changeset intended to be included in the destination|
| `-B`| `--bookmark`| | bookmark to push|
| `-t`| `--to`| | push revs to this bookmark|
| `-d`| `--delete`| | delete remote bookmark|
| | `--create`| | create a new remote bookmark|
| | `--allow-anon`| | allow a new unbookmarked head|
| | `--non-forward-move`| | allows moving a remote bookmark to an arbitrary place|
