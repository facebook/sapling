---
sidebar_position: 40
---
# Navigation

Sapling’s emphasis on editing stacks of commits means users move between commits more often than with other version control systems. To make this easy and intuitive, Sapling provides a number of ways to move around your repository.

### Goto

`sl goto COMMIT` or `sl go COMMIT` is the standard way to checkout a commit in your repository.

```sl-shell-example
# The '@' indicates your currently checked out commit.
$ sl
@  b84224608  13 minutes ago  remote/main
╷
╷ o  15de72785  35 seconds ago  mary  my_feature
╭─╯  Implement glorious features
│
o  a555d064c  Wednesday at 09:06
│
~

$ sl goto 15de72785

$ sl
o  b84224608  13 minutes ago  remote/main
╷
╷ @  15de72785  35 seconds ago  mary  my_feature
╭─╯  Implement glorious features
│
o  a555d064c  Wednesday at 09:06
│
~
```

The argument passed to `sl goto` can be one of the following:

* A full 40-character commit hash, such as `b8422460814900d8f978a8a34a99ae83c6735a70`.
* A short, unique-prefix commit hash, such as `b84224608`.
* A local bookmark name, such as `my_feature` in the example above.
* A remote bookmark name, such as `main` in the example above. Note, the `remote/` prefix is optional.
* A revset query (see below).

#### Auto-pull

By default, Sapling only clones the `main` bookmark of a repository.  Even if you don’t have the remote bookmark locally yet, you can do `sl goto remote/other_bookmark` and it will automatically pull and checkout the remote bookmark for you.

To trigger an auto-pull, you must specify the `remote/` prefix.

### Next/Prev

When working with a stack of commits, you can use `sl next` and `sl prev` to move up and down your stack with ease.

```sl-shell-example
# The '@' indicates your currently checked out commit.
$ sl
o  5abffb82f  Wednesday at 09:39  remote/main
╷
╷ @  824cbba75  13 minutes ago  mary
╷ │  [eden] Support long paths in Windows FSCK
╷ │
╷ o  19340c083  Wednesday at 09:39  mary
╷ │  [eden] Close Windows file handle during Windows Fsck
╷ │
╷ o  b52192598  Wednesday at 09:39  mary
╭─╯  [eden] Use PathMap for WindowsFsck
│
o  2ac18611a  Wednesday at 05:00  remote/stable
╷
~

# Move down the stack from 824cbba75 to 19340c083.
$ sl prev

# Move back up the stack to 824cbba75.
$ sl next

# Move down the stack 2 commits to b52192598.
$ sl prev 2
```

Note, if a commit has multiple children or parents, `next` and `prev` may alert you and you have to choose.

### Top/Bottom

When in a stack, you can jump directly to the top or bottom using `sl goto top` and `sl goto bottom`.

```sl-shell-example
# The '@' indicates your currently checked out commit.
$ sl
o  5abffb82f  Wednesday at 09:39  remote/main
╷
╷ @  824cbba75  13 minutes ago  mary
╷ │  [eden] Support long paths in Windows FSCK
╷ │
╷ o  19340c083  Wednesday at 09:39  mary
╷ │  [eden] Close Windows file handle during Windows Fsck
╷ │
╷ o  b52192598  Wednesday at 09:39  mary
╭─╯  [eden] Use PathMap for WindowsFsck
│
o  2ac18611a  Wednesday at 05:00  remote/stable
╷
~

# Move down to b52192598 at the bottom.
$ sl goto bottom

# Move back up to 824cbba75 at the top.
$ sl goto top
```

### Revsets

`sl goto REVSET` can use the revset query language to specify a commit to go to. See the Revset documentation for more details.

Example revsets:

* `.`  Your current commit.
* `.^` Parent of your current commit.
* `824cbba75~2` Second ancestor of `824cbba75`.
* `19340c083~-1` Child of `19340c083`.
* `ancestor(., main)` The first common ancestor of your current commit and main (that is, `2ac18611a` in the examples above).
