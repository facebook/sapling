---
sidebar_position: 30
---
# Hide/Unhide

One of the classic difficulties of modern version control systems
is figuring out how to undo your mistakes. While Sapling contains
a variety of commands to help you undo mistakes, its foundational
feature is that commits can be hidden and unhidden, and that
doing so is safe, easy to understand, and easy to undo.


While commits can be hidden manually, they can also become hidden
when they are superceded by changes to the commit, whether by
amends, rebases, or other operations. But the key concept is that
they are still stored permanently in your repository, even after
they are no longer visible in your `smartlog` output. If you ever
want to bring back an old commit, it’s as easy as `sl unhide`.

And with a robust `sl undo` command, you can quickly and easily
get to a previous state. Being able to undo just about any
command gives users the confidence to try new commands and learn
from their mistakes.


`sl hide COMMIT` and `sl unhide COMMIT` can be used to simply and
safely hide and recover commits.


```sl-shell-example
$ sl
@  b84224608  Yesterday at 16:04  john  remote/main
│  Updating submodules
~

$ sl unhide 15de72785

$ sl
@  b84224608  Yesterday at 16:04  john  remote/main
╷  Updating submodules
╷
╷ o  15de72785  Yesterday at 16:16  mary
╭─╯  Implement glorious features
│
o  a555d064c  Wednesday at 09:06
│
~

# Note, a555d064c was not unhidden. Smartlog just chose to show it
# so you can see how 15de72785 relates to the main bookmark.

$ sl hide 15de72785

$ sl
@  b84224608  Yesterday at 16:04  john  remote/main
│  Updating submodules
~
```

Notable features:

* You can only hide local commits (known as “draft” commits). You cannot hide commits on bookmarks that came from the server (known as “public” commits).
* If you hide a commit that has commits on top of it (that is, its descendants), all of the commits will be hidden. If you unhide a commit that has commits beneath it (that is, its ancestors), all of the commits will be unhidden.
* You can hide/unhide many commits at once using revsets (see `sl help revset`). For instance, `sl hide "draft()"` will hide all of your local commits.
* Many commands work even on hidden commits. For instance, `sl show COMMIT` will let you inspect a commit before unhiding it. You can even checkout a hidden commit to temporarily work with it.
* Hiding a commit will also remove any local bookmarks on that commit.

There is currently no way to permanently delete a commit from your local repository other than by deleting your repo and recloning.

#### How to find a hidden commit

There are a variety of ways to find which hidden commit you want to unhide.

* `sl smartlog --rev "hidden()"` to view all hidden commits using the “hidden()” revset.
* `sl log -r "predecessors(COMMIT)"` to view the hidden previous versions of a certain commit (that is, the version from before a rebase, amend, etc).
* See [Undo](undo.md) and [Journal](../commands/journal.md) for more ways to view your past repository actions.

