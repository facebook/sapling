---
sidebar_position: 14
---

## githelp | git
<!--
  @generated SignedSource<<a4b7771e3e88e6f887138b22173bde9b>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**suggests the Sapling equivalent of the given git command**

Usage: sl githelp -- $COMMAND

Example:

$ sl git -- checkout my_file.txt baef1046b

sl revert -r my_file.txt baef1046b

The translation is best effort, and if an unknown command or parameter
combination is detected, it simply returns an error.


