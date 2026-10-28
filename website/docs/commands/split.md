---
sidebar_position: 39
---

## split | spl
<!--
  @generated SignedSource<<36b7eb31a531e6c2ddea4f93655f4c76>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**split a commit into smaller commits**

Prompt for hunks to be selected until exhausted. Each selection of hunks
will form a separate commit, in order from parent to child: the first
selection will form the first commit, the second selection will form
the second commit, and so on.

Operates on the current revision by default. Use `--rev` to split a given
commit instead.

Each new commit starts with the original commit message in your editor.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-r`| `--rev`| | revision to split|
| | `--no-rebase`| `false`| don&#x27;t rebase descendants after split|
| `-m`| `--message`| | use text as commit message|
| `-l`| `--logfile`| | read commit message from file|
| `-d`| `--date`| | record the specified date as commit date|
| `-u`| `--user`| | record the specified user as committer|
