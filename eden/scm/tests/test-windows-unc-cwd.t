#require windows

  $ configure modernclient

Make sure things work when CWD is a UNC path.
  $ newclientrepo a
  $ cd \\\\?\\$TESTTMP\\a
  $ touch foo
  $ hg commit -Aq -m foo
