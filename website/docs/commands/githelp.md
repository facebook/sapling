---
sidebar_position: 14
---

## githelp | git
<!--
  @generated SignedSource<<e7647757f621f8d4649ab549a8d09e86>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


suggests the Sapling equivalent of the given git command

Usage: sl githelp -- $COMMAND

Example:

$ sl git -- checkout my_file.txt baef1046b

sl revert -r my_file.txt baef1046b

The translation is best effort, and if an unknown command or parameter
combination is detected, it simply returns an error.

## arguments
no arguments
