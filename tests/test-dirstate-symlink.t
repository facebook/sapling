#require symlink

#testcases v0 v1 v2

#if v0
  $ setconfig format.dirstate=0
#endif

#if v1
  $ setconfig format.dirstate=1
#endif

#if v2
  $ setconfig format.dirstate=2
#endif

  $ newrepo
  $ mkdir a b
  $ touch a/x

  $ hg ci -m init -A a/x

Replace the directory with a symlink

  $ mv a/x b/x
  $ rmdir a
  $ ln -s b a

"! a/x" should be shown, as it is implicitly removed

  $ hg status
  ! a/x
  ? a
  ? b/x

  $ hg ci -m rename -A .
  adding a
  removing a/x
  adding b/x


#if fsmonitor
BUG: fsmonitor "hg status" shows a/x

  $ hg status
  ? a/x
#else
  $ hg status
#endif
