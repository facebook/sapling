---
sidebar_position: 43
---

## uncommit | unc
<!--
  @generated SignedSource<<8d5a8b56541b9a6060455facea60ea63>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**uncommit part or all of the current commit**

Reverse the effects of an `sl commit` operation. When run with no
arguments, hides the current commit and checks out the parent commit,
but does not revert the state of the working copy. Changes that were
contained in the uncommitted commit become pending changes in the
working copy.

`sl uncommit` cannot be run on commits that have children. In
other words, you cannot uncommit a commit in the middle of a
stack. Similarly, by default, you cannot run `sl uncommit` if
there are pending changes in the working copy.

You can selectively uncommit files from the current commit by optionally
specifying a list of files to remove. The specified files are removed from
the list of changed files in the current commit, but are not modified on
disk, so they appear as pending changes in the working copy.

Running `sl uncommit` is similar to running `sl undo --keep`
immediately after `sl commit`. However, unlike `sl undo`, which can
only undo a commit if it was the last operation you performed,
`sl uncommit` can uncommit any draft commit in the graph that does
not have children.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| | `--keep`| `false`| allow an empty commit after uncommiting|
| `-I`| `--include`| | include files matching the given patterns|
| `-X`| `--exclude`| | exclude files matching the given patterns|
