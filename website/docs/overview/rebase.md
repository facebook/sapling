---
sidebar_position: 70
---

# Rebase

While the word "rebase" is a bit confusing, it fundamentally just means moving
commits from one part of the repository to another. In Sapling, this is
done with the `sl rebase` command.

Rebasing requires two pieces of information:
1. Which commits will be moved, via one of the flags `-r` (revisions), `-s` (source), or  `-b` (base).
2. The destination commit to move the commits to, via the `-d`
   (destination) flag.


### Examples
To illustrate the different types of rebases, assume we start with the following
commit graph. Note that we are currently on Commit C, as indicated by the `@`
symbol.
```sl-shell-example
$ sl
o  d78f66e01  106 seconds ago  remote/main
╷
╷ o  306ce5ffb  67 seconds ago  mary
╷ │  Commit F
╷ │
╷ o  2ff5f28a1  67 seconds ago  mary
╷ │  Commit E
╷ │
╷ │ o  e5513dac4  67 seconds ago  mary
╷ │ │  Commit D
╷ │ │
╷ │ @  6f782187a  67 seconds ago  mary
╷ ├─╯  Commit C
╷ │
╷ o  3beadc099  67 seconds ago  mary
╷ │  Commit B
╷ │
╷ o  b48ce5c8e  67 seconds ago  mary
╭─╯  Commit A
│
o  17af69994  Today at 08:33  remote/stable
╷
~
```

#### -b / --base
The most common use for rebase is to move your in-progress commits onto a newer
version of main.  The `-b` flag is ideal for this situation, as it will take the
entire subtree of your commits and move them onto the destination.

Below we use `-b .` to rebase the current commit, `.`, and its entire related
subtree onto main. In this case, the subtree is Commit A and all of its
descendants.

Note that `sl rebase -b . -d XXX` is the same as `sl rebase -d XXX`, as `-b .`
is the default behavior.

```sl-shell-example
# Move entire current subtree onto main.
$ sl rebase -b . -d main
  o  91ecebda8  12 seconds ago  mary
  │  Commit F
  │
  o  b26b55434  12 seconds ago  mary
  │  Commit E
  │
  │ o  a9ef0ee2d  12 seconds ago  mary
  │ │  Commit D
  │ │
  │ @  37d2d0296  12 seconds ago  mary
  ├─╯  Commit C
  │
  o  37d536158  12 seconds ago  mary
  │  Commit B
  │
  o  81910236f  12 seconds ago  mary
╭─╯  Commit A
│
o  d78f66e01  15 minutes ago  remote/main
╷
o  17af69994  Today at 08:33  remote/stable
╷
~
```

#### -s / --source
If you don't want to rebase the entire subtree with `-b`, you can use `-s` to
rebase a given commit and all of its descendants.

Below we use `-s .` to rebase the current commit, `.`, and its descendant
`Commit D` onto `main`. All other commits are left behind.

```sl-shell-example
# Move current commit and its descendants onto main.
$ sl rebase -s . -d main
  o  aa40b4d44  44 seconds ago  mary
  │  Commit D
  │
  @  95cf1e999  44 seconds ago  mary
╭─╯  Commit C
│
o  d78f66e01  11 minutes ago  remote/main
╷
╷ o  306ce5ffb  11 minutes ago  mary
╷ │  Commit F
╷ │
╷ o  2ff5f28a1  11 minutes ago  mary
╷ │  Commit E
╷ │
╷ o  3beadc099  11 minutes ago  mary
╷ │  Commit B
╷ │
╷ o  b48ce5c8e  11 minutes ago  mary
╭─╯  Commit A
│
o  17af69994  Today at 08:33  remote/stable
╷
~
```

#### -r / --revisions
If we only want to move a specific commit or commits, we use `-r` to move
exactly the commits we specified.

Below we move the commit we're on by specifying `.` as the argument to `-r`.  We
move it onto `main` by specifying `main` as the argument to `-d`.

```sl-shell-example
# Move just the current commit to be based on main
$ sl rebase -r . -d main
rebasing 6f782187aa42 "Commit C"
6f782187aa42 -> 8abef7d37f3a "Commit C"
$ sl
  @  8abef7d37  6 seconds ago  mary
╭─╯  Commit C
│
o  d78f66e01  4 minutes ago  remote/main
╷
╷ o  306ce5ffb  3 minutes ago  mary
╷ │  Commit F
╷ │
╷ o  2ff5f28a1  3 minutes ago  mary
╷ │  Commit E
╷ │
╷ │ o  e5513dac4  3 minutes ago  mary
╷ │ │  Commit D
╷ │ │
╷ │ x  6f782187a [Rebased to 8abef7d37f3a]  3 minutes ago  mary
╷ ├─╯  Commit C
╷ │
╷ o  3beadc099  3 minutes ago  mary
╷ │  Commit B
╷ │
╷ o  b48ce5c8e  3 minutes ago  mary
╭─╯  Commit A
│
o  17af69994  Today at 08:33  remote/stable
╷
~
```

Note how `6f782187a` is identified with an `x` in the commit graph to denote that it
has been rewritten to the newer version `8abef7d37f3a`.  However, it is still visible
because commit D did not get rebased.


#### Other
The rebase command can move multiple stacks and subtrees in a single invocation. The tree structure will be retained across the rebase. For example, one can use the `draft()` revset to rebase all of your local commits onto a newer base commit.

```sl-shell-example
$ sl
o  b5d600552  27 seconds ago  remote/main
╷
╷ o  9d5ba71bc  2 minutes ago  mary
╷ │  Commit C
╷ │
╷ │ o  37d536158  12 minutes ago  mary
╷ ├─╯  Commit B
╷ │
╷ o  81910236f  12 minutes ago  mary
╭─╯  Commit A
│
o  d78f66e01  27 minutes ago
╷
╷ @  ecf227650  69 seconds ago  mary
╷ │  Commit Two
╷ │
╷ o  5da2b3e5a  91 seconds ago  mary
╭─╯  Commit One
│
o  17af69994  Today at 08:33  remote/stable
╷
~

# Rebase all my commits onto new main.
$ sl rebase -r 'draft()' -d main

$ sl
  o  3c5549fb7  10 seconds ago  mary
  │  Commit C
  │
  │ o  ed3106510  10 seconds ago  mary
  ├─╯  Commit B
  │
  o  23a6f6012  10 seconds ago  mary
╭─╯  Commit A
│
│ @  9f73762dd  10 seconds ago  mary
│ │  Commit Two
│ │
│ o  9d550d707  10 seconds ago  mary
├─╯  Commit One
│
o  b5d600552  3 minutes ago  remote/main
╷
o  17af69994  Today at 08:33  remote/stable
╷
~
```
