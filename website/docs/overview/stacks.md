---
sidebar_position: 60
---
import {Command} from '@site/elements'

# Stacks of commits

Sapling provides first-class support for editing and manipulating stacks of commits.

### Amend

You can edit any commit in your stack by going to that commit (via <Command name="goto" />), making the desired modifications, and then running <Command name="amend" /> to edit the commit. Keep in mind that if you make a mistake, you can always [Undo](undo.md) your changes!


```sl-shell-example
$ sl
  o  d9a5aa3c7  3 seconds ago  mary
  │  feature two
  │
  @  8a644a0fd  30 seconds ago  mary
╭─╯  feature one
│
o  ea609e1ef  7 minutes ago  remote/main
╷
~

$ echo "Add feature to myproject" >> myproject.cpp

# Apply changes to the current commit
$ sl amend
$ sl show .
...
diff --git a/myproject.cpp b/myproject.cpp
new file mode 100644
--- /dev/null
+++ b/myproject.cpp
@@ -0,0 +1,1 @@
+<amended feature one impl>

```

### Fold

If you have a stack of commits, you can fold commits down into a single commit with the <Command name="fold" /> command. You can either specify `--from <commit id>` to specify a range of commits from your current commit to fold together or specify `--exact <list of commit ids>` to specify exact adjacent commits to fold together.

```sl-shell-example
$ sl
  o  5dbd8043f  82 seconds ago  mary
  │  commit five
  │
  @  bd057eb7f  93 seconds ago  mary
  │  commit four
  │
  o  b65a4efb1  113 seconds ago  mary
  │  commit three
  │
  o  ef7915cd2  2 minutes ago mary
  │  commit two
  │
  o  398748c95  2 minutes ago  mary
╭─╯  commit one
│
o  ea609e1ef  Today at 14:34  remote/main
╷
~

$ sl fold --from ef7915cd2
# (equivalent to sl fold --exact ef7915cd2 b65a4efb1 bd057eb7f)
 3 changesets folded
 update complete
 rebasing 5dbd8043fd7e "commit five"
 merging myproject.cpp
 5dbd8043fd7e -> debf0c562f6e "commit five"

$ sl
  o  debf0c562  9 minutes ago  mary
  │  commit five
  │
  @  3cf9adf66  2 minutes ago  mary
  │  commit two+three+four
  │
  o  398748c95  10 minutes ago  mary
╭─╯  commit one
│
o  ea609e1ef  Today at 14:34  remote/main
╷
~
```


### Split

Use Sapling’s interactive editor interface to split the changes in one commit into two or more smaller commits.


```sl-shell-example
$ sl
  @  b86c5cb40  2 seconds ago  mary
╭─╯  feature one + two
│
o  ea609e1ef  Today at 14:34  remote/main
╷
~

# we want to split apart feature one and feature two
$ sl split
Select hunks to record - [x]=selected **=collapsed c: confirm q: abort
arrow keys: move/expand/collapse space: deselect ?: help
[~] diff --git a/myproject.cpp b/myproject.cpp
 new file mode 100644

 [~] @@ -0,0 +1,3 @@
 [x] +<feature one>
 [ ] +
 [ ] +<feature two>

 # <press c>
 # <enter new commit message for first commit>
Done splitting? [yN] y
# remaining unselected changes go into second commit
# <enter new commit message for second commit>

$ sl
  @  a305c853a  41 seconds ago  mary
  │  feature two
  │
  o  619efe410  2 minutes ago  mary
╭─╯  feature one
│
o  ea609e1ef  Today at 14:34  remote/main
╷
~
```

### Absorb

If you make changes while working at the top of a stack, the <Command name="absorb" /> command allows you to automatically amend those changes to commits lower in the stack. If there is an unambiguous commit which introduced the edited lines, the absorb command will prompt to apply those changes to that commit.

```sl-shell-example
$ sl
  @  a305c853a  41 seconds ago  mary
  │  feature two
  │
  o  619efe410  2 minutes ago  mary
╭─╯  feature one
│
o  ea609e1ef  Today at 14:34  remote/main
╷
~

# Edit part of "feature one", while we are on top of "feature two".
$ vim myproject.cpp
$ sl diff
diff --git a/myproject.cpp b/myproject.cpp
--- a/myproject.cpp
+++ b/myproject.cpp
@@ -1,3 +1,3 @@
-<feature one>
+<modified feature one>

 <feature two>

# Absorb knows that commit 619efe4 introduced the edited lines.
$ sl absorb
showing changes for myproject.cpp
        @@ -0,1 +0,1 @@
619efe4 -<feature one>
619efe4 +<modified feature one>

1 commit affected
619efe4 feature one
apply changes (yn)?  **y**

619efe41024d -> cbf60a27cae4 "feature one"
a305c853a7b5 -> f656ac8c60c8 "feature two"
1 of 1 chunk applied

# Feature one commit now contains the modifications.
$ sl
  @  f656ac8c8  11 seconds ago  mary
  │  feature two
  │
  o  cbf60a274  11 seconds ago  mary
╭─╯  feature one
│
o  ea609e1ef  Today at 14:34  remote/main
╷
~
```

### Amend --to
Sometimes absorb cannot predict an appropriate commit to apply changes to. In this case you can try the command `sl amend --to` to specify exactly which commit to apply pending changes to.

```sl-shell-example
$ sl
  @  f656ac8c6  30 minutes ago  mary
  │  feature two
  │
  o  cbf60a27c  30 minutes ago  mary
╭─╯  feature one
│
o  ea609e1ef  Yesterday at 14:34  remote/main
╷
~

# Add new file for feature one.
$ vim myproject2.cpp
$ sl addremove
adding myproject2.cpp

$ sl diff
diff --git a/myproject2.cpp b/myproject2.cpp
new file mode 100644
--- /dev/null
+++ b/myproject2.cpp
@@ -0,0 +1,1 @@
+<more pieces of feature one>

# Since the changes are in a new file, absorb can't predict
# which commit to apply any changes to.
$ sl absorb
nothing to absorb

# Use 'amend --to' to specify which commit to amend.
$ sl amend --to cbf60a27c
cbf60a27cae4 -> 768f3b26abc3 "feature one"
f656ac8c60c8 -> fe792a25079d "feature two"
```
