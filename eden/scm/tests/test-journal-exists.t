#chg-compatible
#debugruntest-compatible
  $ configure modernclient
  $ newclientrepo repo
  $ echo a > a
  $ hg ci -Am0
  adding a
  $ hg push -q -r . --to book --create

  $ newclientrepo foo test:repo_server book
  $ cd ../repo

  $ echo something > .hg/store/journal

  $ echo foo > a
  $ hg ci -Am0
  abort: abandoned transaction found!
  (run 'hg recover' to clean up transaction)
  [255]

  $ hg recover
  rolling back interrupted transaction
  couldn't read journal entry 'something\n'!

Empty journal is cleaned up automatically.
  $ touch .hg/store/journal
  $ hg ci -Am0
  cleaning up empty abandoned transaction
  rolling back interrupted transaction
