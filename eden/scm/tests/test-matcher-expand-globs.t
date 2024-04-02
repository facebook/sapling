#debugruntest-compatible

We only expand globs on Windows.
#require windows no-eden

  $ configure modernclient
  $ newclientrepo
  $ touch foo foo2 bar
  $ hg st 'foo*'
  ? foo
  ? foo2
