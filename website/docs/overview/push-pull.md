---
sidebar_position: 80
---
# Pull / push

### Pull

The `sl pull` command is used to download new commits from the server. By
default it only pulls remote bookmarks that you are subscribed to. To subscribe
to more bookmarks use `sl pull -B other_bookmark_name`.

`sl pull` only downloads commits. It does not rebase or merge anything, unless
you specify `--rebase`.

Note, this is different from `git pull` which generally pulls all branches and
automatically tries to merge/rebase your changes with the new branches.

```sl-shell-example
$ sl
  @  9f73762dd  62 minutes ago  mary
  │  Commit Two
  │
  o  9d550d707  62 minutes ago  mary
╭─╯  Commit One
│
o  b5d600552  65 minutes ago  remote/main
╷
~

# Fetch new commits from main
$ sl pull

# The current stack is now behind main
$ sl
o  08a7511cc  33 seconds ago  remote/main
╷
╷ @  9f73762dd  63 minutes ago  mary
╷ │  Commit Two
╷ │
╷ o  9d550d707  63 minutes ago  mary
╭─╯  Commit One
│
o  b5d600552  66 minutes ago
╷
~

$ sl pull --rebase
$ sl
  @  e75394bbb  9 seconds ago  mary
  │  Commit Two
  │
  o  4eefdfe1d  9 seconds ago  mary
╭─╯  Commit One
│
o  59125794a  20 seconds ago  remote/main
╷
~
```

### Push

Use the push command to push local commits to remote. Specify the `--to` to specify the remote branch/bookmark to push commits to. Specify `-r` to specify local commit you want pushed. If `-r` is ommitted, the currently checked out commit is pushed.

```sl-shell-example
# Push current commit stack to the remote main bookmark.
$ sl push -r . --to main
```

During a normal `git push` or an `sl push` to a Git repository, the push simply
sends the commits to the server and moves the branch or bookmark forward to the
newly pushed commit.

When `sl push` is used with the Sapling server, `sl push --to BOOKMARK` pushes the
commits to the server and the server additionally rebases them onto the
destination bookmark.  This allows organizations with high push rates to avoid
races where someone is unable to push because someone else pushed first. This
server side rebase is simplified, in that it does not do any file content merging.
If someone else touched the same files you touched, your push will fail and you
will need to pull, rebase, and push again.


Notable push options (some of these may not work when working with Git repos):

- `-r` / `--rev` Specifies which commit to push. Ancestors of this commit will
  also be pushed. Note, only one head can be pushed at a time, so you can't push
  2+ branches at once.
- `--to` Specifies the remote bookmark to push onto.
- `--non-forward-move` Indicates that the push will result in the remote
  bookmark moving backwards or sideways, instead of moving forward to a
  descendant. This is used when a bookmark is in the wrong location and needs to
  be forced elsewhere.
- `--create` Used to indicate the bookmark being pushed to is new and should
  be created.
- `-d` / `--delete` Allows deleting a remote bookmark.
