#debugruntest-compatible

#require no-eden

  $ enable amend copytrace rebase
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

Rebase with fastcopytrace hits conflicts as it doesn't detect the dir rename.
  $ hg rebase --restack
  rebasing 75532295d2d9 "move directory"
  local [dest] changed dir1/file2 which other [source] deleted (as dir1/file1)
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ hg rebase --abort
  rebase aborted

# dagcopytrace does not support directory move
  $ hg rebase --restack --config copytrace.dagcopytrace=True
  rebasing 75532295d2d9 "move directory"
  local [dest] changed dir1/file2 which other [source] deleted (as dir1/file1)
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
