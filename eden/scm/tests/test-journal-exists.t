#debugruntest-compatible

#require no-eden

  $ configure modernclient
  $ newclientrepo repo
  $ echo a > a
  $ hg ci -Am0
  adding a
  $ hg push -q -r . --to book --create

  $ newclientrepo foo test:repo_server book
  $ cd ../repo

Journal is cleaned up automatically.
  $ echo something > .hg/store/journal

  $ echo foo > a
  $ hg ci -Am0
  couldn't read journal entry 'something\n'!

  $ hg recover
  no interrupted transaction available
  [1]

Empty journal is cleaned up automatically.
  $ touch .hg/store/journal
  $ hg ci -Am0
  nothing changed
  [1]
