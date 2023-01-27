#require git no-windows
#debugruntest-compatible

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
  diff --git a/a b/a
  deleted file mode 100644
  --- a/a
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -1
  diff --git a/b b/b
  new file mode 100644
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,1 @@
  +1
