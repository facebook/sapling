#chg-compatible
#debugruntest-compatible

  $ . "$TESTDIR/library.sh"

  $ hg init test
  $ cd test
  $ echo foo > file
  $ hg commit -Aqm "Add file"

Adding a symlink without the extension works
  $ ln -s file link
  $ f link
  link -> file
  $ hg add link
  $ hg commit -m "Add link"

Adding a symlink with the extension works also
  $ ln -s file link2
  $ f link2
  link2 -> file
  $ hg --config extensions.disablesymlinks= add link2
  $ hg --config extensions.disablesymlinks= commit -m "Add link2"

Checking out a commit with the extension does not produce a symlink
  $ hg checkout null
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ hg checkout tip --config extensions.disablesymlinks=
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ f file link link2
  file
  link
  link2
  $ cat link
  file (no-eol)
