---
sidebar_position: 23
---

## next | n | ne | nex
<!--
  @generated SignedSource<<e06e380ee422a49ed20b284a338f0ff0>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**check out a child commit**

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| | `--newest`| `false`| always pick the newest child when a changeset has multiple children|
| | `--rebase`| `false`| rebase each changeset if necessary|
| | `--top`| `false`| update to the head of the current stack|
| | `--bookmark`| `false`| update to the first changeset with a bookmark|
| | `--no-activate-bookmark`| `false`| do not activate the bookmark on the destination changeset|
| | `--towards`| | move linearly towards the specified head|
| `-C`| `--clean`| `false`| discard uncommitted changes (no backup)|
| `-B`| `--move-bookmark`| `false`| move active bookmark|
| `-m`| `--merge`| `false`| merge uncommitted changes|
| `-c`| `--check`| `false`| require clean working directory|
