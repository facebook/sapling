---
sidebar_position: 13
---

## forget
<!--
  @generated SignedSource<<6cb7f5eec202a3ac6f0afead91e22fc7>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**stop tracking the specified files**

Mark the specified files so they will no longer be tracked
after the next commit.

Forget does not delete the files from the working copy. To delete
the file from the working copy, see `sl remove`.

Forget does not remove files from the repository history. The files
will only be removed in the next commit and its descendants.

To undo a forget before the next commit, see `sl add`.

Examples:

- forget newly-added binary files:

```
sl forget "set:added() and binary()"
```

- forget files that would be excluded by .gitignore:

```
sl forget "set:gitignore()"
```

Returns 0 on success.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-I`| `--include`| | include files matching the given patterns|
| `-X`| `--exclude`| | exclude files matching the given patterns|
