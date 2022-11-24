---
sidebar_position: 50
---

import {Command} from '@site/elements'

# Bookmarks

Sapling has bookmarks instead of branches. Like a Git branch, a
bookmark is just a name that points to a particular commit and
can be used to refer to that commit during checkout, rebase, etc.
Sapling has two different types of bookmarks: local and remote.

Sapling does not require local bookmarks for development. In
fact, for day-to-day development, bookmarks are discouraged since
people find it easier not to use them.


Local bookmarks:

* Exist only on your local machine.
* Can be seen and modified only by you, using the `sl bookmark` command.
* Are completely optional, and generally unnecessary for normal workflows.
* Can be active or inactive (see below). Active means they move when you commit.

Remote bookmarks:

* Are local copies of the server’s bookmarks.
* Appear prefixed with `remote/` (example: `remote/main`)
* Cannot be modified locally by you.  They can only be updated by `sl pull` or moved by `sl push`.


All of your local bookmarks and your most important remote
bookmarks are shown in `smartlog` output. In this example,
`remote/main` is a remote bookmark and `my_feature` is created as
a local bookmark.


```sl-shell-example
$ sl
o  b84224608  13 minutes ago  remote/main
╷
╷ @  15de72785  35 seconds ago  mary
╭─╯  Implement glorious features
│
o  a555d064c  Wednesday at 09:06
│
~

$ sl bookmark my_feature

$ sl
o  b84224608  13 minutes ago  remote/main
╷
╷ @  15de72785  35 seconds ago  mary  my_feature*
╭─╯  Implement glorious features
│
o  a555d064c  Wednesday at 09:06
│
~
```

#### Active bookmarks

The asterisk (`*`) on the `my_feature` bookmark indicates the
bookmark is active. If you make a commit, the active bookmark
will move forward to point at the new commit. If `my_feature` was
not active, making a new commit would not change `my_feature`,
which would remain pointing at `15de72785`.

A bookmark is made active when it is first created or when it is
explicitly checked out with `sl goto my_feature`. It is made
inactive when you explicitly go to another commit, like with `sl
goto OTHER_COMMIT`.


### Differences from Git branches

* In Git you must always be working on a local branch, otherwise
  you end up in the confusing "detached head" state and any
  commits you make may be hard to find later or may be garbage
  collected. In Sapling, you do not need a bookmark when creating
  a new commit, and commits are visible in `smartlog` regardless
  of whether they have a bookmark or not.
* In Git, deleting a branch makes it difficult to find the
  commits that were on that branch. In Sapling, deleting a local
  bookmark doesn’t hide commits, and bookmarks are safe and
  easy to add and delete.
* In Git, rebasing or amending a branch will only affect that
  branch. If multiple branches were pointing at the same
  commit, the other branches will be left behind, still
  pointing at the old commit. In Sapling, when you rebase or
  amend, every bookmark on a commit will be moved to the new
  version of the commit.
