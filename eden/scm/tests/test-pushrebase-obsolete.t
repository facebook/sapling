#chg-compatible
#debugruntest-compatible
  $ setconfig experimental.allowfilepeer=True

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
  │
  │ o  C
  │ │
  x │  B
  ├─╯
  o  A
  

  $ hg bookmark -r $D foo
  $ hg push -r $D --to foo --create
  pushing rev be0ef73c17ad to destination ssh://user@dummy/server bookmark foo
  searching for changes
  exporting bookmark foo
  remote: pushing 3 changesets:
  remote:     426bada5c675  A
  remote:     112478962961  B
  remote:     be0ef73c17ad  D

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
  abort: commits already rebased to destination as dc0947a82db8
  [255]
