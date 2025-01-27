---
sidebar_position: 34
---

## restack
<!--
  @generated SignedSource<<5dc5e20c918b52b4a4a3a5565aed6ba0>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


rebase all commits in the current stack onto the latest version of their respective parents

`restack` is a built-in alias for `rebase --restack`

When commits are modified by commands like `amend` and `absorb`, their descendant
commits may be left behind as orphans. Rebase these orphaned commits onto the newest
versions of their ancestors, making the stack linear again.


