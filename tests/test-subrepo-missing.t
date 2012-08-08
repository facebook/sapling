  $ hg init repo
  $ cd repo
  $ hg init subrepo
  $ echo a > subrepo/a
  $ hg -R subrepo ci -Am adda
  adding a
  $ echo 'subrepo = subrepo' > .hgsub
  $ hg ci -Am addsubrepo
  adding .hgsub
  $ echo b > subrepo/b
  $ hg -R subrepo ci -Am addb
  adding b
  $ hg ci -m updatedsub

ignore blanklines in .hgsubstate

  >>> file('.hgsubstate', 'wb').write('\n\n   \t \n   \n')
  $ hg st --subrepos
  M .hgsubstate
  $ hg revert -qC .hgsubstate

abort more gracefully on .hgsubstate parsing error

  $ cp .hgsubstate .hgsubstate.old
  >>> file('.hgsubstate', 'wb').write('\ninvalid')
  $ hg st --subrepos
  abort: invalid subrepository revision specifier in .hgsubstate line 2
  [255]
  $ mv .hgsubstate.old .hgsubstate

delete .hgsub and revert it

  $ rm .hgsub
  $ hg revert .hgsub
  warning: subrepo spec file .hgsub not found
  warning: subrepo spec file .hgsub not found

delete .hgsubstate and revert it

  $ rm .hgsubstate
  $ hg revert .hgsubstate

delete .hgsub and update

  $ rm .hgsub
  $ hg up 0
  warning: subrepo spec file .hgsub not found
  warning: subrepo spec file .hgsub not found
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg st
  warning: subrepo spec file .hgsub not found
  ! .hgsub
  $ ls subrepo
  a

delete .hgsubstate and update

  $ hg up -C
  warning: subrepo spec file .hgsub not found
  warning: subrepo spec file .hgsub not found
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm .hgsubstate
  $ hg up 0
  remote changed .hgsubstate which local deleted
  use (c)hanged version or leave (d)eleted? c
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg st
  $ ls subrepo
  a

  $ cd ..
