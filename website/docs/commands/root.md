---
sidebar_position: 34
---

## root
<!--
  @generated SignedSource<<6c030be04949fdc237d0af6105d916c9>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**print the repository's root (top) of the current working directory**

Print the root directory of the current repository.

Frequently useful in shells scripts and automation to run commands like:

```
$  ./$(sl root)/bin/script.py
```

Returns 0 on success.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| | `--shared`| `false`| show root of the shared repo|
