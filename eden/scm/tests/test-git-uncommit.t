#require git no-windows no-eden
#debugruntest-compatible

  $ eagerepo
  $ . $TESTDIR/git.sh

Prepare repo

  $ hg init --git repo1
  $ cd repo1
  $ echo 'A--B' | drawdag
  $ hg up -q $B

Test uncommit

  $ enable amend
  $ hg uncommit

  $ hg st
  A B

  $ hg log -r. -T '{desc}\n'
  A

