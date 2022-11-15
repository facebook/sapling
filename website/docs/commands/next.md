---
sidebar_position: 25
---

## next
<!--
  @generated SignedSource<<fcf053c7e8e05d1ac6289c2cd1569273>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**check out a descendant commit**

Update to a descendant commit of the current commit. When working with a stack
of commits, you can use `sl next` to move up your stack with ease.

- Use the `--newest` flag to always pick the newest of multiple child commits.  You can set `amend.alwaysnewest` to true in your global Sapling config file  to make this the default.

- Use the `--merge` flag to bring along uncommitted changes to the destination  commit.

- Use the `--bookmark` flag to move to the next commit with a bookmark.

- Use the `--rebase` flag to rebase any child commits that were left behind  after `amend`, `split`, `fold`, or `histedit`.

Examples:

- Move 1 level up the stack:

```
sl next
```

- Move 2 levels up the stack:

```
sl next 2
```

- Move to the top of the stack:

```
sl next --top
```

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| | `--newest`| `false`| always pick the newest child when a commit has multiple children|
| | `--rebase`| `false`| rebase each commit if necessary|
| | `--top`| `false`| update to the head of the current stack|
| | `--bookmark`| `false`| update to the first commit with a bookmark|
| | `--no-activate-bookmark`| `false`| do not activate the bookmark on the destination commit|
| | `--towards`| | move linearly towards the specified head|
| `-C`| `--clean`| `false`| discard uncommitted changes (no backup)|
| `-B`| `--move-bookmark`| `false`| move active bookmark|
| `-m`| `--merge`| `false`| merge uncommitted changes|
| `-c`| `--check`| `false`| require clean working directory|
