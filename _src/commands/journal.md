---
sidebar_position: 22
---

## journal | jo
<!--
  @generated SignedSource<<0e7de1e471e68d39910eea9d2c9c6638>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**show the history of the checked out commit or a bookmark**

Show the history of all the commits that were once the current commit. In
other words, shows a list of your previously checked out commits.
`sl journal` can be used to find older versions of commits (for example,
when you want to revert to a previous state). It can also be used to
discover commits that were previously hidden.

By default, `sl journal` displays the history of the current commit. To
display a list of commits pointed to by a bookmark, specify a bookmark
name.

Specify `--all` to show the history of both the current commit and all
bookmarks. In the output for `--all`, bookmarks are listed by name, and
`.` indicates the current commit.

Specify `-Tjson` to produce machine-readable output.

By default, `sl journal` only shows the commit hash and the
corresponding command. Specify `--verbose` to also include the
previous commit hash, user, and timestamp.

Use `-c/--commits` to output log information about each commit
hash. To customize the log output, you can also specify switches
like `--patch`, `git`, `--stat`, and `--template`.

If a bookmark name starts with `re:`, the remainder of the name
is treated as a regular expression. To match a name that actually
starts with `re:`, use the prefix `literal:`.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| | `--all`| | show history for all names|
| `-c`| `--commits`| | show commit metadata|
| `-p`| `--patch`| | show patch|
| `-g`| `--git`| | use git extended diff format|
| `-l`| `--limit`| | limit number of changes displayed|
| | `--stat`| | output diffstat-style summary of changes|
| `-T`| `--template`| | display with template|
