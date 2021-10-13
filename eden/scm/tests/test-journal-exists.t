#chg-compatible
  $ configure modernclient
  $ newclientrepo repo
  $ echo a > a
  $ hg ci -Am0
  adding a
  $ hg push -q -r . --to book --create

  $ newclientrepo foo test:repo_server book
  $ cd ../repo

  $ touch .hg/store/journal

  $ echo foo > a
  $ hg ci -Am0
  abort: abandoned transaction found!
  (run 'hg recover' to clean up transaction)
  [255]

  $ hg recover
  rolling back interrupted transaction
