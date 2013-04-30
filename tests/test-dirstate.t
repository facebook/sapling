------ Test dirstate._dirs refcounting

  $ hg init t
  $ cd t
  $ mkdir -p a/b/c/d
  $ touch a/b/c/d/x
  $ touch a/b/c/d/y
  $ touch a/b/c/d/z
  $ hg ci -Am m
  adding a/b/c/d/x
  adding a/b/c/d/y
  adding a/b/c/d/z
  $ hg mv a z
  moving a/b/c/d/x to z/b/c/d/x (glob)
  moving a/b/c/d/y to z/b/c/d/y (glob)
  moving a/b/c/d/z to z/b/c/d/z (glob)

Test name collisions

  $ rm z/b/c/d/x
  $ mkdir z/b/c/d/x
  $ touch z/b/c/d/x/y
  $ hg add z/b/c/d/x/y
  abort: file 'z/b/c/d/x' in dirstate clashes with 'z/b/c/d/x/y'
  [255]
  $ rm -rf z/b/c/d
  $ touch z/b/c/d
  $ hg add z/b/c/d
  abort: directory 'z/b/c/d' already in dirstate
  [255]

  $ cd ..

Issue1790: dirstate entry locked into unset if file mtime is set into
the future

Prepare test repo:

  $ hg init u
  $ cd u
  $ echo a > a
  $ hg add
  adding a
  $ hg ci -m1

Set mtime of a into the future:

  $ touch -t 202101011200 a

Status must not set a's entry to unset (issue1790):

  $ hg status
  $ hg debugstate
  n 644          2 2021-01-01 12:00:00 a

Test modulo storage/comparison of absurd dates:

#if no-aix
  $ touch -t 195001011200 a
  $ hg st
  $ hg debugstate
  n 644          2 2018-01-19 15:14:08 a
#endif
