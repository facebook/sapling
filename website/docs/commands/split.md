---
sidebar_position: 35
---

## split | spl | spli
<!--
  @generated SignedSource<<0f4d611d700a6f718084a2a487d4f4dc>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**split a changeset into smaller changesets**

Prompt for hunks to be selected until exhausted. Each selection of hunks
will form a separate changeset, in order from parent to child: the first
selection will form the first changeset, the second selection will form
the second changeset, and so on.

Operates on the current revision by default. Use --rev to split a given
changeset instead.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-r`| `--rev`| | revision to split|
| | `--no-rebase`| `false`| don't rebase descendants after split|
| `-m`| `--message`| | use text as commit message|
| `-l`| `--logfile`| | read commit message from file|
| `-d`| `--date`| | record the specified date as commit date|
| `-u`| `--user`| | record the specified user as committer|
