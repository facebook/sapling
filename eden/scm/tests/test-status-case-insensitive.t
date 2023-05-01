#debugruntest-compatible
#require icasefs
#require no-windows

  $ eagerepo

  $ newclientrepo
  $ touch foo
  $ hg commit -Aqm foo
  $ mv foo FOO
Don't show file as "!"
  $ hg st
