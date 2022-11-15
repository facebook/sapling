---
sidebar_position: 36
---

## show
<!--
  @generated SignedSource<<da4f2cd812e2b9229ac71773b9bafeb5>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**show commit in detail**

Show the commit message and contents for the specified commit. If no commit
is specified, shows the current commit.

`sl show` behaves similarly to `sl log -vp -r REV [OPTION]... [FILE]...`, or
if called without a `REV`, `sl log -vp -r . [OPTION]...` Use
`sl log` for more powerful operations than supported by `sl show`.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| | `--nodates`| | omit dates from diff headers (but keeps it in commit header)|
| | `--noprefix`| | omit a/ and b/ prefixes from filenames|
| | `--stat`| | output diffstat-style summary of changes|
| `-g`| `--git`| | use git extended diff format|
| `-U`| `--unified`| `3`| number of lines of diff context to show|
| `-w`| `--ignore-all-space`| | ignore white space when comparing lines|
| `-b`| `--ignore-space-change`| | ignore changes in the amount of white space|
| `-B`| `--ignore-blank-lines`| | ignore changes whose lines are all blank|
| `-Z`| `--ignore-space-at-eol`| | ignore changes in whitespace at EOL|
| `-T`| `--template`| | display with template|
| `-I`| `--include`| | include files matching the given patterns|
| `-X`| `--exclude`| | exclude files matching the given patterns|
