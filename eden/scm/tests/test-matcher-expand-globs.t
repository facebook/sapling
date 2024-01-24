#debugruntest-compatible

We only expand globs on Windows.
#require windows

  $ configure modernclient
  $ newclientrepo
  $ touch foo foo2 bar
  $ hg st 'foo*'
  ? foo
  ? foo2
