#debugruntest-compatible
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

Rebase with fastcopytrace hits conflicts as it doesn't detect the dir rename.
  $ hg rebase --restack
  rebasing 75532295d2d9 "move directory"
  local [dest] changed dir1/file2 which other [source] deleted (as dir1/file1)
  hint: if this message is due to a moved file, you can ask mercurial to attempt to automatically resolve this change by re-running with the --config=experimental.copytrace=on flag, but this will significantly slow down the operation, so you will need to be patient.
  Source control team is working on fixing this problem.
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ hg rebase --abort
  rebase aborted

Using full copytrace works.
  $ hg rebase --restack --config experimental.copytrace=on
  rebasing 75532295d2d9 "move directory"
