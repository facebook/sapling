#chg-compatible
#require symlink
#debugruntest-compatible

  $ setconfig workingcopy.ruststatus=False
  $ hg init repo
  $ cd repo
  $ mkdir foo
  $ touch foo/file
  $ hg commit -m one -A
  adding foo/file
  $ mkdir bar
  $ touch bar/file
  $ rm -rf foo
  $ ln -s bar foo
  $ hg addremove
  adding bar/file
  adding foo
  removing foo/file

Don't get confused by foo/file reapparing behind the symlink.
  $ hg addremove
