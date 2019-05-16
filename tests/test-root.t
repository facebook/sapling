make shared repo

  $ enable share
  $ newrepo repo1
  $ echo a > a
  $ hg commit -q -A -m 'init'
  $ cd $TESTTMP
  $ hg share -q repo1 repo2
  $ cd repo2

test repo --shared

  $ hg root --shared
  $TESTTMP/repo1
