---
sidebar_position: 36
---

## shelve
<!--
  @generated SignedSource<<c104bf82a72985c470946dcc453e060f>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**save pending changes and revert working copy to a clean state**

Shelving takes files that `sl status` reports as not clean, saves
the modifications to a bundle (a shelved change), and reverts the
files to a clean state in the working copy.

To restore the changes to the working copy, using `sl unshelve`,
regardless of your current commit.

When no files are specified, `sl shelve` saves all not-clean
files. If specific files or directories are named, only changes to
those files are shelved.

Each shelved change has a name that makes it easier to find later.
The name of a shelved change by default is based on the active
bookmark. To specify a different name, use `--name`.

To see a list of existing shelved changes, use the `--list`
option. For each shelved change, this will print its name, age,
and description. Use `--patch` or `--stat` for more details.

To delete specific shelved changes, use `--delete`. To delete
all shelved changes, use `--cleanup`.

Returns 0 on success.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-A`| `--addremove`| | mark new/missing files as added/removed before shelving|
| `-u`| `--unknown`| | store unknown files in the shelve|
| | `--cleanup`| | delete all shelved changes|
| | `--date`| | shelve with the specified commit date|
| `-d`| `--delete`| | delete the named shelved change(s)|
| `-e`| `--edit`| `false`| invoke editor on commit messages|
| `-l`| `--list`| | list current shelves|
| `-m`| `--message`| | use text as shelve message|
| `-n`| `--name`| | use the given name for the shelved commit|
| `-p`| `--patch`| | show patch|
| `-i`| `--interactive`| | interactive mode - only works while creating a shelve|
| | `--stat`| | output diffstat-style summary of changes|
| `-I`| `--include`| | include files matching the given patterns|
| `-X`| `--exclude`| | exclude files matching the given patterns|
