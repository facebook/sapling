---
sidebar_position: 26
---

## previous | prev
<!--
  @generated SignedSource<<a7b33aeefc757172e1b4ae955581d2fe>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**check out an ancestor commit**

Update to an ancestor commit of the current commit. When working with a stack
of commits, you can use `sl previous` to move down your stack with ease.

- Use the `--newest` flag to always pick the newest of multiple parents commits.  You can set `amend.alwaysnewest` to true in your global Sapling config file to make  this the default.

- Use the `--merge` flag to bring along uncommitted changes to the destination  commit.

- Use the `--bookmark` flag to move to the first ancestor commit with a bookmark.

Examples:

- Move 1 level down the stack:

```
sl prev
```

- Move 2 levels down the stack:

```
sl prev 2
```

- Move to the bottom of the stack:

```
sl prev --bottom
```

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| | `--newest`| `false`| always pick the newest parent when a commit has multiple parents|
| | `--bottom`| `false`| update to the lowest non-public ancestor of the current commit|
| | `--bookmark`| `false`| update to the first ancestor with a bookmark|
| | `--no-activate-bookmark`| `false`| do not activate the bookmark on the destination commit|
| `-C`| `--clean`| `false`| discard uncommitted changes (no backup)|
| `-B`| `--move-bookmark`| `false`| move active bookmark|
| `-m`| `--merge`| `false`| merge uncommitted changes|
| `-c`| `--check`| `false`| require clean working directory|
