#debugruntest-compatible

  $ configure modernclient
  $ newclientrepo
  $ echo foo > .gitignore
Avoid dirstate race condition where added files end up as NEED_CHECK.
  $ sleep 1
  $ hg commit -Aqm ignore
  $ mkdir foo
  $ touch foo/a foo/b foo/c
  $ hg add -q foo/a foo/b foo/c

  $ rm foo/a
  $ hg st
  A foo/b
  A foo/c
  ! foo/a

  $ hg st -A
  A foo/b
  A foo/c
  ! foo/a
  C .gitignore

  $ hg st -i

  $ hg st -ai
  A foo/b
  A foo/c

  $ hg forget -q foo

We want the ignore files to be present in our treestate.
  $ hg debugtree list
  .gitignore: 0100644 4 + EXIST_P1 EXIST_NEXT  (no-windows !)
  .gitignore: 0100666 4 + EXIST_P1 EXIST_NEXT  (windows !)
  foo/a: 00 -1 -1 NEED_CHECK  (fsmonitor !)
  foo/b: 00 -1 -1 NEED_CHECK  (fsmonitor !)
  foo/c: 00 -1 -1 NEED_CHECK  (fsmonitor !)

#if fsmonitor
We shouldn't need to check any files from treestate.
  $ LOG=workingcopy::watchmanfs=debug hg status 2>&1 | grep treestate_needs_check
  * treestate_needs_check=0 (glob)
#endif
