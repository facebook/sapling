#chg-compatible
#require fsmonitor

  $ setconfig status.use-rust=true workingcopy.use-rust=true

  $ configure modernclient
  $ newclientrepo
  $ echo foo > .gitignore
  $ hg commit -Aqm ignore
  $ mkdir foo
  $ touch foo/a foo/b foo/c
  $ hg add -q foo/a foo/b foo/c
  $ hg forget -q foo

We want the ignore files to be present in our treestate.
  $ hg debugtree list
  .gitignore: 0100644 4 + EXIST_P1 EXIST_NEXT 
  foo/a: 00 -1 -1 NEED_CHECK 
  foo/b: 00 -1 -1 NEED_CHECK 
  foo/c: 00 -1 -1 NEED_CHECK 

We shouldn't need to check any files from treestate.
  $ LOG=workingcopy::watchmanfs::state=debug hg status 2>&1 | grep treestate_needs_check
  DEBUG * watchman_needs_check=1 treestate_needs_check=0 (glob)
