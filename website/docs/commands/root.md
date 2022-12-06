---
sidebar_position: 35
---

## root
<!--
  @generated SignedSource<<956ef71abc94ca44b3d3c2832682f99a>>
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
