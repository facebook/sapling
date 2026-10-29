---
sidebar_position: 43
---

## uncommit | unc
<!--
  @generated SignedSource<<4d5fe6010ce1121b97f6aa6ed64ee28d>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**uncommit part or all of the current commit**

Reverse the effects of an `sl commit` operation. When run with no
arguments, hides the current commit and checks out the parent commit,
but does not revert the state of the working copy. Changes that were
contained in the uncommitted commit become pending changes in the
working copy.

The current commit can be in the middle of a stack. If some of its
changes are uncommitted, or all of them with `--keep`, the commit is
rewritten and its descendants are restacked onto the new version as
`sl amend` restacks them (see `amend.autorestack`). Descendants
whose restack would conflict are left on the old version; run
`sl restack` to move them. Uncommitting every change of a commit
that has descendants requires `--keep`, which leaves the commit in the
stack, empty: without it, nothing would replace the commit.

By default, you cannot run `sl uncommit` if there are pending changes
in the working copy.

You can selectively uncommit files from the current commit by optionally
specifying a list of files to remove. The specified files are removed from
the list of changed files in the current commit, but are not modified on
disk, so they appear as pending changes in the working copy.

Running `sl uncommit` is similar to running `sl undo --keep`
immediately after `sl commit`. However, unlike `sl undo`, which can
only undo a commit if it was the last operation you performed,
`sl uncommit` can uncommit any draft commit you check out.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| | `--keep`| `false`| allow an empty commit after uncommiting|
| `-I`| `--include`| | include files matching the given patterns|
| `-X`| `--exclude`| | exclude files matching the given patterns|
