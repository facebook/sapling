---
sidebar_position: 16
---

## githelp | git
<!--
  @generated SignedSource<<d9c70ba4260b498b22e0a91c0392d4e6>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**suggests the Sapling equivalent of the given git command**

Usage: sl githelp -- $COMMAND

Example:

$ sl git -- checkout my_file.txt baef1046b

sl revert -r my_file.txt baef1046b

The translation is best effort, and if an unknown command or parameter
combination is detected, it simply returns an error.


