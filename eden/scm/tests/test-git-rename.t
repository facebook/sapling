#require git no-windows no-eden
#debugruntest-compatible

  $ eagerepo
  $ . $TESTDIR/git.sh
  $ setconfig diff.git=True

Prepare repo

  $ hg init --git repo1
  $ cd repo1
  $ echo 1 >> a
  $ hg ci -q -Am 'add a'

Test mv

  $ hg mv a b
  $ hg ci -Am 'mv a -> b'
  $ hg diff -r.~1 -r .
  diff --git a/a b/b
  rename from a
  rename to b
