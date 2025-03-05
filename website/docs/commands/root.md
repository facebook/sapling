---
sidebar_position: 35
---

## root
<!--
  @generated SignedSource<<2a12bbb018c5e8a2b49840d7a20c763c>>
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
