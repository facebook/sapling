
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
  .gitignore: * 4 + EXIST_P1 EXIST_NEXT  (glob) (no-eden !)
  foo/a: 00 -1 -1 NEED_CHECK  (fsmonitor !)
  foo/b: 00 -1 -1 NEED_CHECK  (fsmonitor !)
  foo/c: 00 -1 -1 NEED_CHECK  (fsmonitor !)
