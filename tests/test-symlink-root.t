  $ "$TESTDIR/hghave" symlink || exit 80

  $ hg init a
  $ ln -s a link
  $ cd a
  $ echo foo > foo
  $ hg status
  ? foo
  $ hg status ../link
  ? foo
