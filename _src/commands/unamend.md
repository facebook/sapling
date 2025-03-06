---
sidebar_position: 42
---

## unamend | una
<!--
  @generated SignedSource<<1ac9652d2016c5018141767ffd7b8d7e>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**undo the last amend operation on the current commit**

Reverse the effects of an `sl amend` operation. Hides the current commit
and checks out the previous version of the commit. `sl unamend` does not
revert the state of the working copy, so changes that were added to the
commit in the last amend operation become pending changes in the working
copy.

`sl unamend` cannot be run on amended commits that have children. In
other words, you cannot unamend an amended commit in the middle of a
stack.

Running `sl unamend` is similar to running `sl undo --keep`
immediately after `sl amend`. However, unlike `sl undo`, which can
only undo an amend if it was the last operation you performed,
`sl unamend` can unamend any draft amended commit in the graph that
does not have children.

Although `sl unamend` is typically used to reverse the effects of
`sl amend`, it actually rolls back the current commit to its previous
version, regardless of whether the changes resulted from an `sl amend`
operation or from another operation, such as `sl rebase`.


