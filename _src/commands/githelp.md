---
sidebar_position: 15
---

## githelp | git
<!--
  @generated SignedSource<<eca63d413bdb08f690b17f75ce93ca0a>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**suggests the Sapling equivalent of the given git command**

Usage: sl githelp -- $COMMAND

Example:

$ sl git -- checkout my_file.txt baef1046b

sl revert -r my_file.txt baef1046b

The translation is best effort, and if an unknown command or parameter
combination is detected, it simply returns an error.


