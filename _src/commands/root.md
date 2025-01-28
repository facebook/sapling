---
sidebar_position: 36
---

## root
<!--
  @generated SignedSource<<83e850fadac793d85e396bdf210737cb>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**print the repository&#x27;s root (top) of the current working directory**

Print the root directory of the current repository.

Frequently useful in shells scripts and automation to run commands like:

```
$ $(sl root)/bin/script.py
```

Returns 0 on success.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| | `--shared`| `false`| show root of the shared repo|
