#chg-compatible
#debugruntest-compatible

  $ configure modernclient

Prepare Repo:

  $ newclientrepo repo
  $ setconfig paths.default=test:e1
  $ drawdag << 'EOS'
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

  $ hg push -r $E --to master --create -q

Clone the lazy repo:

  $ newclientrepo cloned1 test:e1

Commit and edit on top of B:

  $ LOG=dag::protocol=debug,checkout::prefetch=debug hg up $B -q
  DEBUG dag::protocol: resolve names [112478962961147124edd43549aedd1a335e44bf] remotely
  DEBUG dag::protocol: resolve ids [2] remotely
  DEBUG checkout::prefetch: children of 112478962961147124edd43549aedd1a335e44bf: ['26805aba1e600a82e93661149f2313866a221a7b']
  DEBUG dag::protocol: resolve ids [3] remotely
  $ touch B1
  $ LOG=dag::protocol=debug hg commit -Am B1 B1

  $ LOG=dag::protocol=debug hg metaedit -m B11
