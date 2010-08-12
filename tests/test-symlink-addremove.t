  $ "$TESTDIR/hghave" symlink || exit 80

  $ hg init a
  $ cd a

directory moved and symlinked

  $ mkdir foo
  $ touch foo/a
  $ hg ci -Ama
  adding foo/a
  $ mv foo bar
  $ ln -s bar foo

now addremove should remove old files

  $ hg addremove
  adding bar/a
  adding foo
  removing foo/a
