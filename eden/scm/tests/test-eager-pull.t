#chg-compatible

  $ configure modern

  $ setconfig paths.default=test:e1

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
  $ LOG=pull::fastpath=debug hg pull
  pulling from test:e2
  DEBUG pull::fastpath: master: 26805aba1e600a82e93661149f2313866a221a7b => 9bc730a19041f9ec7cb33c626e811aa233efb18c
  imported commit graph for 2 commits (1 segment)
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
  
(pull again does not trigger pull fast path API)
  $ LOG=pull::fastpath=debug hg pull
  pulling from test:e2
  DEBUG pull::fastpath: master: 9bc730a19041f9ec7cb33c626e811aa233efb18c (unchanged)

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
  $ LOG=pull::fastpath=debug hg pull
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
  $ FAILPOINTS=transaction-metalog-commit=return LOG=pull::fastpath=debug hg pull
  pulling from test:e2
  DEBUG pull::fastpath: master: 26805aba1e600a82e93661149f2313866a221a7b => 9bc730a19041f9ec7cb33c626e811aa233efb18c
  imported commit graph for 2 commits (1 segment)
  abort: failpoint 'transaction-metalog-commit' set by FAILPOINTS
  transaction abort!
  rollback completed
  [255]

Fast path can still be used with stale remotenames:

  $ setconfig paths.default=test:e3
  $ LOG=pull::fastpath=debug,dag::protocol=debug  hg pull
  pulling from test:e3
  DEBUG pull::fastpath: master: 9bc730a19041f9ec7cb33c626e811aa233efb18c => 7b3a68e117f183a6da8e60779d8fbeeed22382bb
  DEBUG dag::protocol: resolve names [9d37022187178c68e8fe8dff17c9c57fb62b9ea5, a194cadd16930608adaa649035ad4c16930cbd0f] remotely
  imported commit graph for 5 commits (3 segments)

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
  
If fast path is broken, use fallback pull path:

  $ newremoterepo
  $ setconfig paths.default=test:e1
  $ hg debugchangelog --migrate lazy
  $ hg pull -qB master

  $ setconfig paths.default=test:e2
  $ FAILPOINTS=eagerepo::api::pulllazy=return LOG=pull::fastpath=debug hg pull
  pulling from test:e2
  DEBUG pull::fastpath: master: 26805aba1e600a82e93661149f2313866a221a7b => 9bc730a19041f9ec7cb33c626e811aa233efb18c
  failed to get fast pull data (Not supported by the server), using fallback path
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
  
