---
sidebar_position: 7
---

## cat
<!--
  @generated SignedSource<<ae5c33df272def0a9e30fcbf7d936a7b>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**output the current or given revision of files**

Print the specified files as they were at the given revision. If
no revision is given, the parent of the working directory is used.

Output may be to a file, in which case the name of the file is
given using a format string. The formatting rules as follows:

`%%`
literal "%" character

`%s`
basename of file being printed

`%d`
dirname of file being printed, or '.' if in repository root

`%p`
root-relative path name of file being printed

`%H`
changeset hash (40 hexadecimal digits)

`%R`
changeset revision number

`%h`
short-form changeset hash (12 hexadecimal digits)

`%r`
zero-padded changeset revision number

`%b`
basename of the exporting repository

Returns 0 on success.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-o`| `--output`| | print output to file with formatted name|
| `-r`| `--rev`| | print the given revision|
| `-I`| `--include`| | include files matching the given patterns|
| `-X`| `--exclude`| | exclude files matching the given patterns|
