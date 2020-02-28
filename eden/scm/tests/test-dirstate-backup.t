#chg-compatible

#chg-compatible

#chg-compatible

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

Set up

  $ hg init repo
  $ cd repo

Try to import an empty patch

  $ hg import --no-commit - <<EOF
  > EOF
  applying patch from stdin
  abort: stdin: no diffs found
  [255]

No dirstate backups are left behind

  $ ls .hg/dirstate* | sort
  .hg/dirstate
  .hg/dirstate.tree.* (glob) (?)

