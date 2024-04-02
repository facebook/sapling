#debugruntest-compatible

#require symlink no-eden

  $ eagerepo

  $ hg init unix-repo
  $ cd unix-repo
  $ echo foo > a
  $ ln -s a b
  $ hg ci -Am0
  adding a
  adding b
  $ cd ..

Simulate a checkout shared on NFS/Samba:

  $ hg clone -q unix-repo shared
  $ cd shared
  $ rm b
  $ echo foo > b
  $ SL_DEBUG_DISABLE_SYMLINKS=1 hg status --debug
  ignoring suspect symlink placeholder "b" (?)

Make a clone using placeholders:

  $ SL_DEBUG_DISABLE_SYMLINKS=1 hg clone . ../win-repo
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../win-repo
  $ cat b
  a (no-eol)
  $ SL_DEBUG_DISABLE_SYMLINKS=1 hg st --debug

Empty placeholder:

  $ rm b
  $ touch b
  $ SL_DEBUG_DISABLE_SYMLINKS=1 hg st --debug
  ignoring suspect symlink placeholder "b" (?)

Write binary data to the placeholder:

  >>> _ = open('b', 'w').write('this is a binary\0')
  $ SL_DEBUG_DISABLE_SYMLINKS=1 hg st --debug
  ignoring suspect symlink placeholder "b" (?)

Write a long string to the placeholder:

  >>> _ = open('b', 'w').write('this' * 1000)
  $ SL_DEBUG_DISABLE_SYMLINKS=1 hg st --debug
  ignoring suspect symlink placeholder "b" (?)

Commit shouldn't succeed:

  $ SL_DEBUG_DISABLE_SYMLINKS=1 hg ci -m1
  nothing changed
  [1]

Write a valid string to the placeholder:

  >>> open('b', 'w').write('this')
  4
  $ SL_DEBUG_DISABLE_SYMLINKS=1 hg st --debug
  M b
  $ SL_DEBUG_DISABLE_SYMLINKS=1 hg ci -m1
  $ hg manifest tip --verbose
  644   a
  644 @ b

  $ cd ..
