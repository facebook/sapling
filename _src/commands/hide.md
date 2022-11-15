---
sidebar_position: 18
---

## hide
<!--
  @generated SignedSource<<490e37a16a361287b8aaf9963e05f9a7>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**hide commits and their descendants**

Mark the specified commits as hidden. Hidden commits are not included in
the output of most Sapling commands, including `sl log` and
`sl smartlog.` Any descendants of the specified commits will also be
hidden.

Hidden commits are not deleted. They will remain in the repo indefinitely
and are still accessible by their hashes. However, `sl hide` will delete
any bookmarks pointing to hidden commits.

Use the `sl unhide` command to make hidden commits visible again. See
`sl help unhide` for more information.

To view hidden commits, run `sl journal`.

When you hide the current commit, the most recent visible ancestor is
checked out.

To hide obsolete stacks (stacks that have a newer version), run
`sl hide --cleanup`. This command is equivalent to:

`sl hide 'obsolete() - ancestors(draft() & not obsolete())'`

`--cleanup` skips obsolete commits with non-obsolete descendants.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-r`| `--rev`| | revisions to hide|
| `-c`| `--cleanup`| | clean up commits with newer versions, and non-essential remote bookmarks|
| `-B`| `--bookmark`| | hide commits only reachable from a bookmark|
