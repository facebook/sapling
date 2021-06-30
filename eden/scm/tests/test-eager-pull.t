#chg-compatible

  $ configure modern

  $ setconfig paths.default=test:e1
  $ setconfig treemanifest.flatcompat=0

Disable SSH:

  $ setconfig ui.ssh=false

Prepare Repo:

  $ newremoterepo
  $ setconfig paths.default=test:e1
  $ drawdag <<'EOS'
  > H
  > |\
  > G I
  > | |
  > F |
  > | J
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

  $ hg push -r $C --to master --create -q
  $ hg push -r $E --to master --create -q --config paths.default=test:e2
  $ hg push -r $H --to master --create -q --config paths.default=test:e3

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
  added 2 commits lazily (1 segments)
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
  added 2 commits lazily (1 segments)
  abort: injected error by tests: transaction-metalog-commit
  transaction abort!
  rollback failed - please run hg recover
  [255]
  $ hg recover
  rolling back interrupted transaction

Fast path can still be used with stale remotenames:

  $ setconfig paths.default=test:e3
  $ LOG=pull::fastpath=debug,dag::protocol=debug  hg pull --config pull.master-fastpath=True
  pulling from test:e3
   DEBUG pull::fastpath: master: 9bc730a19041f9ec7cb33c626e811aa233efb18c => 7b3a68e117f183a6da8e60779d8fbeeed22382bb
   DEBUG dag::protocol: resolve names [9d37022187178c68e8fe8dff17c9c57fb62b9ea5, a194cadd16930608adaa649035ad4c16930cbd0f] remotely
  added 5 commits lazily (3 segments)

  $ EDENSCM_DISABLE_REMOTE_RESOLVE=0000000000000000000000000000000000000000 LOG=dag::protocol=debug hg log -Gr 'all()' -T '{desc} {remotenames}'
  o    H remote/master
  ├─╮
  │ o  I
  │ │
  │ o  J
  │
  o  G
  │
  o  F
  │
  o  E
  │
  o  D
  │
  o  C
  │
  o  B
  │
  o  A
  
