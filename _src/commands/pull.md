---
sidebar_position: 28
---

## pull
<!--
  @generated SignedSource<<6aff9248b359c19059123b5153495d02>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**pull commits from the specified source**

Pull commits from a remote repository to a local one. This command modifies
the commit graph, but doesn't mutate local commits or the working copy.

Use `-B/--bookmark` to specify a remote bookmark to pull. For Git
repos, remote bookmarks correspond to branches. If no bookmark is
specified, a default set of relevant remote names are pulled.

If SOURCE is omitted, the default path is used. Use `sl path
--add` to add a named source.

See `sl help urls` and `sl help path` for more information.

Examples:

- pull relevant remote bookmarks from default source:

```
sl pull
```

- pull a bookmark named my-branch from source my-fork:

sl pull my-fork --bookmark my-branch

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
