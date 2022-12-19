---
sidebar_position: 10
---

import {Command} from '@site/elements'

# Basic commands

Here are the most commonly used commands in Sapling:

### Working with commits

| Get | View | Change | Move | Remove | Fix | Stack |
| ---- | ------ | --------- | --- | --- | ----- | --- |
| <Command name="clone" /> | <Command name="sl" />| <Command name="amend" /> | <Command name="rebase" /> | <Command name="hide" /> | <Command name="uncommit" /> | <Command name="fold" /> |
| <Command name="pull" /> | <Command name="show" /> | <Command name="metaedit" /> | <Command name="graft" /> | <Command name="unhide" /> | <Command name="unamend" /> | <Command name="split" /> |
| | <Command name="log" /> | | | | <Command name="undo" /> | <Command name="absorb" /> |
| | <Command name="web" />| | | | <Command name="redo" /> | <Command name="histedit" /> |
| | | | | | | <Command name="restack" /> |

### Working with your checkout

| View | Move | Change | Fix | Save |
| ---- | --- | ------ | --- | --- |
| <Command name="status" /> | <Command name="goto" /> | <Command name="add" /> | <Command name="revert" /> | <Command name="commit" /> |
| <Command name="diff" /> | <Command name="next" /> | <Command name="remove" /> | <Command name="clean" /> | <Command name="shelve" /> |
| | <Command name="prev" /> | <Command name="forget" /> | | |
| | | <Command name="move" /> | | |
| | | <Command name="copy" /> | | |


# Examples

Many of Sapling’s basic commands will be familiar, and perhaps even unremarkable, to existing Git and Mercurial users.  For example, Sapling supports <Command name="clone" />, <Command name="checkout" />, <Command name="commit" />, <Command name="rebase" />, <Command name="push" />, etc. The goal was not to reinvent the wheel, but to make an intuitive, yet powerful, source control system.

This document is a casual introduction to some of the basic commands. It is not comprehensive, nor is it a walkthrough of an end-to-end workflow (see the [Introduction](../introduction/introduction.md) for a simple, end-to-end example). Commands with interesting nuance or for more advanced cases are covered in other documents.

Many of these examples use the `sl smartlog` output to explain the repo state. See the [Smartlog doc](./smartlog) for an overview of the output format.

## Cloning and checking out

#### Clone

Clone the repo using the `sl clone` command.

```sl-shell-example
# Clones into a 'sapling' directory.
$ sl clone https://github.com/facebook/sapling
remote: Enumerating objects: 640374, done.
remote: Counting objects: 100% (5233/5233), done.
remote: Compressing objects: 100% (3228/3228), done.
remote: Total 640374 (delta 1749), reused 5139 (delta 1669), pack-reused 635141
Receiving objects: 100% (640374/640374), 155.18 MiB | 15.17 MiB/s, done.
Resolving deltas: 100% (431325/431325), done.
From https://github.com/facebook/sapling
 * [new ref]               b8422460814900d8f978a8a34a99ae83c6735a70 -> remote/main
5689 files updated, 0 files merged, 0 files removed, 0 files unresolved

# Clones into a 'some_directory' directory.
$ sl clone https://github.com/facebook/sapling some_directory
```

:::note

For Git support, Sapling uses Git under the hood for clones, pushes, and pulls. Hence the output matches Git.

:::

Related topics: [Push/Pull](push-pull.md), Sparse

#### Goto/Checkout

`sl goto` or `sl go` allows you to checkout a specific commit.  See the [Navigation](navigation.md) document for a variety of other ways to move around your repository.

```sl-shell-example
# You can checkout commits by their long or short hash.
# '@' in smartlog indicates your current checkout location.
$ sl goto 71f7ac009
$ sl
o  b84224608  13 minutes ago  remote/main
╷
@  71f7ac009  Today at 10:10  john
╷  scsc: fix build on Windows
╷
╷ o  15de72785  35 seconds ago  mary  my_feature
╭─╯  Implement glorious features
│
o  a555d064c  Wednesday at 09:06
│
~

# You can checkout remote bookmark commits, either by `name` or by `remote/name`.
$ sl goto main

# You can checkout commits pointed at by local bookmarks.
$ sl goto my_feature
```

You can checkout a commit while you have pending changes, as long as the checkout does not change files with pending changes.

Notable options:

* `-C/--clean`  will remove any pending changes.

Related topics: [Navigation](navigation.md), [top/bottom](navigation.md#topbottom), [pull](push-pull.md)

## Working copy

#### Status

`sl status` or `sl st` shows a list of your current uncommitted files.

```sl-shell-example
$ vim build.sh
$ sl st
M build.sh

$ vim new_file.txt
$ sl st
M build.sh
? new_file.txt

# File state indicators:
#   M - modified file
#   A - new file that has been marked with 'sl add'
#   R - deleted file that has been marked with 'sl remove'
#   ! - deleted file that has not yet been marked with 'sl remove'
#   ? - new file that has not yet been marked with 'sl add'
```

Unlike Git, Sapling does not use a staging area, so any non-? files in the status output will be committed when you run `sl commit`.

Notable options:

* `--copies` shows which files have been marked as moved/copied.
* `--change COMMIT` shows the files changed in a given commit.

#### Diff

`sl diff` shows you the diff output for your current uncommitted changes.

```sl-shell-example
$ sl diff
diff --git a/build.sh b/build.sh
--- a/build.sh
+++ b/build.sh
@@ -9,6 +9,10 @@
     PATH="$TOOLCHAIN_DIR:$PATH"
 fi

+if [[ -n $TEST_ENVIRONMENT ]]; then
+    exit 1
+fi
+
 SCRIPT_DIR=$(dirname "${BASH_SOURCE[0]}")

# Specify a file to only see its changes.
$ sl diff file.txt
```

The diff output is compatible with Git’s diff format.

Related topics: [Show](basic-commands.md#show)

#### Add/Remove/Forget

`sl add/remove/forget` are used to add new files, remove old files, and undo added files, respectively. Only files marked M/A/R will be committed during `sl commit`.

```sl-shell-example
$ sl st
? new_file.txt

$ sl add new_file.txt
$ sl st
A new_file.txt

$ rm old_file.txt
$ sl st
A new_file.txt
! old_file.txt

$ sl rm old_file.txt
$ sl st
A new_file.txt
R old_file.txt

$ sl forget new_file.txt
$ sl st
? new_file.txt
R old_file.txt
```

#### Move/Copy

`sl mv/cp` can be used to rename or copy a file.

```sl-shell-example
$ sl mv old_name.txt new_name.txt
$ sl st
A new_name.txt
R old_name.txt

# Sapling-only repos track the move/copy, which can be viewed with sl st --copies.
$ sl st --copies
A new_name.txt
  old_name.txt
R old_name.txt
```

When using Git support, file renames are not recorded since Git does not record this information. When using a normal Sapling repository, the rename/copy will be tracked inside Sapling and used to show accurate log and blame output for the file.

Related topics: AutoMove

#### Clean

`sl clean` deletes any untracked files (`?` in status) in your working copy.

```sl-shell-example
$ sl st
? temp_file

$ sl clean
$ sl st
```

#### Revert

`sl revert` will revert any pending changes in your working copy.

```sl-shell-example
$ sl st
M build.sh

$ sl revert build.sh
$ sl st
```

Notable options:

* `--all` will revert all pending changes, so you don’t need to specify file names.
* `--rev COMMIT` will change the file contents to match their contents in the given commit.
* `--interactive` will open an interactive editor for choosing which files or lines to revert.

## Making commits

#### Commit

`sl commit` commits your pending changes and prompts you for a commit message.  While there is no staging area, the powerful `--interactive` option is used to select specific files or lines you want committed.

```sl-shell-example
$ sl st
M build.sh
$ sl commit
# ...opens your editor so you can write a message...

$ sl
  @  c178f2e7f  1 second ago mary
╭─╯  Fix build.sh
│
o  b84224608  52 minutes ago  remote/main
│
~
```

Notable options:

* `-m/--message MSG` allows specifying a message instead of opening an editor.
* `--interactive` will open an interactive editor for choosing which files or lines to commit.  Lines/files not chosen remain as pending changes.

Related topics: Amend

## Viewing history

Related: [smartlog](smartlog.md)

#### Show

`sl show` shows the log message and textual diff for the current or given commit.

```sl-shell-example
$ sl show
commit:   c178f2e7ff20447532370599051c1f1939f9dcb6   (@)
parent:   b8422460814900d8f978a8a34a99ae83c6735a70
user:     Mary Smith <mary@example.com>
date:     Mon, 15 Aug 2022 16:56:36 -0700

    My new commit

diff --git a/build.sh b/build.sh
--- a/build.sh
+++ b/build.sh
@@ -9,6 +9,10 @@
     PATH="$TOOLCHAIN_DIR:$PATH"
 fi

+if [[ -n $TEST_ENVIRONMENT ]]; then
+    exit 1
+fi
+
 SCRIPT_DIR=$(dirname "${BASH_SOURCE[0]}")

# Can also show a particular commit
$ sl show COMMIT
```

#### Log

`sl log` shows the commit history starting at your current commit.

Unlike in Git and Mercurial, the `log` command in Sapling is rarely used. Instead, `smartlog` is preferred for day-to-day development and understanding your repository.  `sl log` is really only used when inspecting the deeper history of the repository or a file.

```sl-shell-example
$ sl log
changeset:   c178f2e7ff20447532370599051c1f1939f9dcb6   (@)
user:        Mary Smith <mary@example.com>
date:        Mon, 15 Aug 2022 16:56:36 -0700
summary:     My new commit

changeset:   b8422460814900d8f978a8a34a99ae83c6735a70
user:        John Adams <john@example.com>
date:        Mon, 15 Aug 2022 16:04:08 -0700
summary:     globalrevs: lookup globalrevs over edenapi

changeset:   98f29d99b8b8b8a6562e98faa913a650bd0f0302
user:        John Adams <john@example.com>
date:        Mon, 15 Aug 2022 15:14:49 -0700
summary:     remove glob from scuba logging test

changeset:   6bec7b92894495229635f481ba01895c869c2063
user:        Mary Smith <mary@example.com>
date:        Mon, 15 Aug 2022 14:27:14 -0700
summary:     fix monitoring for tailer

# Specify a file or directory to see its history
$ sl log src/build.rs

# Use -fr with a commit to show the history starting from there.
$ sl log -fr COMMIT src/build.rs
```
