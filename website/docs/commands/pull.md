---
sidebar_position: 26
---

## pull
<!--
  @generated SignedSource<<5556f99642a125cde3df0f43a49ec39f>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**pull changes from the specified source**

Pull changes from a remote repository to a local one. This command modifies
the commit graph, but doesn't affect local commits or the working copy.

If SOURCE is omitted, the default path is used.
See `sl help urls` for more information.

You can use `.` for BOOKMARK to specify the active bookmark.

Returns 0 on success, 1 on failure, including if `--update` was
specified but the update had unresolved conflicts.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-u`| `--update`| | update to new branch head if new descendants were pulled|
| `-f`| `--force`| | run even when remote repository is unrelated|
| `-r`| `--rev`| | a remote commit to pull|
| `-B`| `--bookmark`| | a bookmark to pull|
| | `--rebase`| | rebase current commit or current stack onto master|
| `-t`| `--tool`| | specify merge tool for rebase|
| `-d`| `--dest`| | destination for rebase or update|
