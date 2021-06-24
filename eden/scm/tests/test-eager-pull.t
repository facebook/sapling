#chg-compatible

  $ configure modern

  $ setconfig paths.default=test:e1 ui.traceback=1
  $ setconfig treemanifest.flatcompat=0
#   $ export LOG=edenscm::mercurial::eagerpeer=trace,eagerepo=trace

Disable SSH:

  $ setconfig ui.ssh=false

Prepare Repo:

  $ newremoterepo
  $ setconfig paths.default=test:e1
  $ drawdag <<EOS
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ hg push -r $C --to master --create
  pushing rev 26805aba1e60 to destination test:e1 bookmark master
  searching for changes
  exporting bookmark master
  $ newremoterepo
  $ setconfig paths.default=test:e2
  $ drawdag <<EOS
  > E
  > |
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ hg push -r $E --to master --create
  pushing rev 9bc730a19041 to destination test:e2 bookmark master
  searching for changes
  exporting bookmark master

Pull:

  $ newremoterepo
  $ setconfig paths.default=test:e1
  $ hg debugchangelog --migrate lazy
  $ hg pull -r $C
  pulling from test:e1
  $ hg log -Gr 'all()' -T '{desc} {remotenames}'
  o  C remote/master
  │
  o  B
  │
  o  A
  

  $ setconfig paths.default=test:e2
  $ hg debugsegmentpull $C $E
  Got 1 segments and 2 ids
  $ hg log -Gr ::$E -T '{desc} {remotenames}'
  o  E
  │
  o  D
  │
  o  C remote/master
  │
  o  B
  │
  o  A
  
