---
sidebar_position: 4
---

## annotate | blame | an
<!--
  @generated SignedSource<<68dd9e5d01aff54467da88143c5d6825>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**show per-line commit information for given files**

Show file contents where each line is annotated with information
about the commit that last changed that line.

This command is useful for discovering when a change was made and
by whom.

If you include `--file`, `--user`, or `--date`, the revision number is
suppressed unless you also include `--number`.

Without the `-a/--text` option, annotate will skip binary files.
With `-a`, binary files will be annotated anyway.

Returns 0 on success.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-r`| `--rev`| | annotate the specified revision|
| | `--no-follow`| `false`| don't follow copies and renames|
| `-a`| `--text`| | treat all files as text|
| `-u`| `--user`| | list the author (long with -v)|
| `-f`| `--file`| | list the filename|
| `-d`| `--date`| | list the date (short with -q)|
| `-n`| `--number`| | list the revision number|
| `-c`| `--changeset`| | list the changeset (default)|
| `-l`| `--line-number`| | show line number at the first appearance|
| `-w`| `--ignore-all-space`| | ignore white space when comparing lines|
| `-b`| `--ignore-space-change`| | ignore changes in the amount of white space|
| `-B`| `--ignore-blank-lines`| | ignore changes whose lines are all blank|
| `-Z`| `--ignore-space-at-eol`| | ignore changes in whitespace at EOL|
| `-I`| `--include`| | include files matching the given patterns|
| `-X`| `--exclude`| | exclude files matching the given patterns|
| `-p`| `--phabdiff`| | list phabricator diff id|
