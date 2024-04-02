#debugruntest-compatible
#require git no-windows no-eden

  $ . $TESTDIR/git.sh

Prepare a git repo:

  $ git init -q gitrepo
  $ cd gitrepo
  $ git config core.autocrlf false
  $ echo 1 > alpha
  $ git add alpha
  $ git commit -q -malpha

  $ git switch -c foo
  Switched to a new branch 'foo'

  $ echo 2 > beta
  $ git add beta
  $ git commit -q -mbeta

Test git clone sets publicheads
  $ hg clone --git "$TESTTMP/gitrepo" cloned
  From $TESTTMP/gitrepo
   * [new ref]         3f5848713286c67b8a71a450e98c7fa66787bde2 -> remote/foo
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd cloned
  $ hg config remotenames.publicheads
  remote/foo,remote/main,remote/master
