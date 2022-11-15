---
sidebar_position: 29
---

## redo
<!--
  @generated SignedSource<<2d911636af36210f3dd684fbc36c40ff>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**undo the last undo**

Reverse the effects of an `sl undo` operation.

You can run `sl redo` multiple times to undo a series of `sl undo`
commands. Alternatively, you can explicitly specify the number of
`sl undo` commands to undo by providing a number as a positional argument.

Specify `--preview` to see a graphical display that shows what your smartlog
will look like after you run the command.

For an interactive interface, run `sl undo --interactive`. This command
enables you to visually step backwards and forwards in the undo history.
Run `sl help undo` for more information.

Returns 0 on success.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-p`| `--preview`| `false`| see smartlog-like preview of future redo state|
