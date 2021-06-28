#chg-compatible

  $ configure modern

  $ setconfig paths.default=test:e1
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
  $ LOG=pull::fastpath=debug hg pull --config pull.master-fastpath=True
  pulling from test:e2
   DEBUG pull::fastpath: master: 26805aba1e600a82e93661149f2313866a221a7b => 9bc730a19041f9ec7cb33c626e811aa233efb18c
  $ hg log -Gr 'all()' -T '{desc} {remotenames}'
  o  E remote/master
  │
  o  D
  │
  o  C
  │
  o  B
  │
  o  A
  

Test fallback to slow path:

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
  $ drawdag <<EOS
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ LOG=pull::fastpath=debug hg pull --config pull.master-fastpath=True
  pulling from test:e2
   DEBUG pull::fastpath: master: 26805aba1e600a82e93661149f2313866a221a7b => 9bc730a19041f9ec7cb33c626e811aa233efb18c
    WARN pull::fastpath: cannot use pull fast path: NeedSlowPath: f585351a92f85104bff7c284233c338b10eb1df7 exists in local graph
  
  searching for changes
  $ hg log -Gr 'all()' -T '{desc} {remotenames}'
  o  E remote/master
  │
  o  D
  │
  o  C
  │
  o  B
  │
  o  A
  
Test DAG flushed but not metalog (Emulates Ctrl+C or SIGKILL in between):

  $ newremoterepo
  $ setconfig paths.default=test:e1
  $ hg debugchangelog --migrate lazy
  $ hg pull
  pulling from test:e1

  $ setconfig paths.default=test:e2
  $ LOG=pull::fastpath=debug hg pull --config pull.master-fastpath=True --config fault-injection.transaction-metalog-commit=True
  pulling from test:e2
   DEBUG pull::fastpath: master: 26805aba1e600a82e93661149f2313866a221a7b => 9bc730a19041f9ec7cb33c626e811aa233efb18c
  abort: injected error by tests: transaction-metalog-commit
  transaction abort!
  rollback failed - please run hg recover
  [255]
  $ hg recover
  rolling back interrupted transaction

Suboptimal: fast path cannot be used:

  $ LOG=pull::fastpath=debug hg pull --config pull.master-fastpath=True
  pulling from test:e2
   DEBUG pull::fastpath: master: 26805aba1e600a82e93661149f2313866a221a7b => 9bc730a19041f9ec7cb33c626e811aa233efb18c
    WARN pull::fastpath: cannot use pull fast path: NeedSlowPath: f585351a92f85104bff7c284233c338b10eb1df7 exists in local graph
  
  no changes found
