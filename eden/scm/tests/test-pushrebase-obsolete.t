#chg-compatible
#require py2

  $ configure modern
  $ enable pushrebase

Push with obsoleted commits with successors not in the destination.

  $ newserver server
  $ clone server repo1

  $ cd repo1
  $ drawdag << 'EOS'
  > D
  > |
  > B C # amend: B -> C
  >  \|
  >   A
  > EOS

  $ hg log -Gr "all()" -T '{desc}\n'
  o  D
  |
  | o  C
  | |
  x |  B
  |/
  o  A
  

BUG: D should be pushable.
  $ hg bookmark -r $D foo
  $ hg push -r $D --to foo --create
  pushing rev be0ef73c17ad to destination ssh://user@dummy/server bookmark foo
  searching for changes
  abort: cannot rebase obsolete changesets
  [255]

Push with obsoleted commits with successors in the destination.

  $ cd $TESTTMP
  $ newserver server2
  $ clone server2 repo2

  $ cd repo2
  $ drawdag << 'EOS'
  > D E
  > | |
  > B C # amend: B -> C
  >  \|
  >   A
  > EOS

  $ hg bookmark -r $E foo
  $ hg push -r $E --to foo --create -q

  $ hg push -r $D --to foo
  pushing rev be0ef73c17ad to destination ssh://user@dummy/server2 bookmark foo
  searching for changes
  abort: cannot rebase obsolete changesets
  [255]

