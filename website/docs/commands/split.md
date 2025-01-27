---
sidebar_position: 40
---

## split | spl
<!--
  @generated SignedSource<<8199447fee4a04411536d952d648404d>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**split a commit into smaller commits**

Prompt for hunks to be selected until exhausted. Each selection of hunks
will form a separate commit, in order from parent to child: the first
selection will form the first commit, the second selection will form
the second commit, and so on.

Operates on the current revision by default. Use `--rev` to split a given
commit instead.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-r`| `--rev`| | revision to split|
| | `--no-rebase`| `false`| don&#x27;t rebase descendants after split|
| `-m`| `--message`| | use text as commit message|
| `-l`| `--logfile`| | read commit message from file|
| `-d`| `--date`| | record the specified date as commit date|
| `-u`| `--user`| | record the specified user as committer|
