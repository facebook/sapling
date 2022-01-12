  $ enable amend copytrace rebase
  $ setconfig copytrace.fastcopytrace=true experimental.copytrace=off
  $ newrepo

  $ touch base
  $ hg commit -Aqm base

Create a file, and then move it to another directory in the next commit.
  $ mkdir dir1
  $ echo a > dir1/file1
  $ hg commit -Am "original commit"
  adding dir1/file1
  $ mkdir dir2
  $ hg mv dir1/file1 dir2/
  $ hg commit -m "move directory"

Amend the bottom commit to rename the file
  $ hg prev -q
  [54bfd3] original commit
  $ hg mv dir1/file1 dir1/file2
  $ hg amend
  hint[amend-restack]: descendants of 54bfd3f39556 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints

BUG! Rebase with fastcopytrace doesn't work.
  $ hg rebase --restack
  rebasing 75532295d2d9 "move directory"
  abort: dir1/file1@75532295d2d9: not found in manifest!
  [255]

Using full copytrace still works.
  $ hg rebase --continue --config experimental.copytrace=on
  rebasing 75532295d2d9 "move directory"
