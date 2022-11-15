---
sidebar_position: 14
---

## update | up | checkout | co | goto | go
<!--
  @generated SignedSource<<a98c7b3d4b4f0bdd83f390705299454d>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**update working copy to a given commit**

Update your working copy to the given destination commit. More
precisely, make the destination commit the current commit and update the
contents of all files in your working copy to match their state in the
destination commit.

By default, if you attempt to go to a commit while you have pending
changes, and the destination commit is not an ancestor or descendant of
the current commit, the checkout will abort. However, if the destination
commit is an ancestor or descendant of the current commit, the pending
changes will be merged with the destination.

Use one of the following flags to modify this behavior:

```
--check: abort if there are pending changes
```

```
--clean: permanently discard any pending changes (use with caution)
```

```
--merge: always attempt to merge the pending changes into the destination
```

If merge conflicts occur during update, Sapling enters an unfinished
merge state. If this happens, fix the conflicts manually and then run
`sl commit` to exit the unfinished merge state and save your changes
in a new commit. Alternatively, run `sl goto --clean` to discard your
pending changes.

Specify null as the destination commit to get an empty working copy
(sometimes known as a bare repository).

Returns 0 on success, 1 if there are unresolved files.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-C`| `--clean`| `false`| discard uncommitted changes (no backup)|
| `-c`| `--check`| `false`| require clean working copy|
| `-m`| `--merge`| `false`| merge uncommitted changes|
| `-r`| `--rev`| | revision|
| | `--inactive`| `false`| update without activating bookmarks|
| `-B`| `--bookmark`| | create new bookmark|
| `-t`| `--tool`| | specify merge tool|
