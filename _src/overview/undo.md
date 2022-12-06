---
sidebar_position: 90
---

# Undo

Since Sapling keeps a full record of the mutation history of commits, most Sapling commands that modify commits can be easily undone.  The `sl undo` command will revert the commit graph to its state prior to the last run command.

```sl-shell-example
$ sl
  @  e75394bbb  16 minutes ago  mary
  │  Commit Two
  │
  o  4eefdfe1d  16 minutes ago  mary
╭─╯  Commit One
│
o  59125794a  16 minutes ago  remote/main
╷
o  774057207  Today at 10:48  remote/stable
╷
~

# Change #1 to rename the commit.
$ sl metaedit -m "Commit Two Renamed"

# Change #2 to move the commit.
$ sl rebase -s 4eefdfe1d -d stable

$ sl
o  59125794a  24 minutes ago  remote/main
╷
╷ @  c87ec5f32  56 seconds ago  mary
╷ │  Commit Two Renamed
╷ │
╷ o  a5054dd01  56 seconds ago  mary
╭─╯  Commit One
│
o  774057207  Today at 10:48  remote/stable
╷
~

# Undo change #2.
$ sl undo
$ sl
  @  f5c155dd8  2 minutes ago  mary
  │  Commit Two Renamed
  │
  o  4eefdfe1d  25 minutes ago  mary
╭─╯  Commit One
│
o  59125794a  25 minutes ago  remote/main
╷
o  774057207  Today at 10:48  remote/stable
╷
~
```

Running the command again will undo the command run before the last undone command. Use the `sl redo` command to reverse the undo command.


```sl-shell-example
# Undo change #1.
$ sl undo
$ sl
  @  e75394bbb  27 minutes ago  mary
  │  Commit Two
  │
  o  4eefdfe1d  27 minutes ago  mary
╭─╯  Commit One
│
o  59125794a  27 minutes ago  remote/master
╷
~

# Oops! I didn't mean to undo that rename.
$ sl redo
$ sl
$ @  f5c155dd8  5 minutes ago  mary
  │  Commit Two Renamed
  │
  o  4eefdfe1d  28 minutes ago  mary
╭─╯  Commit One
│
o  59125794a  28 minutes ago  remote/main
╷
~
```

#### Undo --interactive

You can visualize the undo before it happens by using the `sl undo -i`
interactive command. This gives an interactive terminal UI where you can use the
left and right keyboard keys to view the previous states you can undo to.

Red commits are those that will be removed, while yellow are commits that will be
visible. Press `enter` to confirm the rollback, or press `q` to abort.

This UI is also useful for simply finding old commit hashes. Once you have the
hash, you can exit the undo UI, then use `sl show HASH` and `sl unhide HASH` to
view and recover the commit.

### Uncommit / unamend

The undo command is limited to undoing changes to the commit graph. To undo changes related to the working copy, like a commit or amend, use `sl uncommit` and `sl unamend`.


```sl-shell-example
$ sl
  @  1a22ba0e9  83 seconds ago  mary
╭─╯  my feature
│
o  59125794a  36 minutes ago  remote/main
╷
~

$ echo "edit myproject.cpp" >> myproject.cpp
$ sl commit -m "new commit"

$ sl
  @  6024e2ffd  14 seconds ago  mary
  │  new commit
  │
  o  1a22ba0e9  3 minutes ago  mary
╭─╯  my feature
│
o  59125794a  38 minutes ago  remote/main
╷
~

# Oops! I meant to amend my changes instead.
$ sl uncommit
  @  1a22ba0e9  4 minutes ago  mary
╭─╯  my feature
│
o  59125794a  39 minutes ago  remote/main
╷
~

# Now we're back to the state prior to the commit.
$ sl st
M myproject.cpp

$ sl amend
$ sl
  @  6ca2114d1  3 seconds ago  mary
╭─╯  my feature
│
o  59125794a  41 minutes ago  remote/main
╷
~

# Now let's say we change our mind and decide to make a new
# commit after all. Let's undo the amend.
$ sl unamend
$ sl st
M myproject.cpp
# now the changes are back as pending changes in our working copy
```

You can limit uncommit to specific files by using `sl uncommit FILE1 FILE2 ...`.
