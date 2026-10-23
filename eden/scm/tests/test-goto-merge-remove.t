#require eden

  $ newclientrepo
  $ touch a b
  $ sl add a b
  $ sl commit -m "commit 1"
  $ sl rm a
  $ sl commit -m "commit 2"
  $ sl go 'desc("commit 1")' -q
  $ sl rm a
  $ sl go 'desc("commit 2")' --merge
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl st
  R a
