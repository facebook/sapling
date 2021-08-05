#chg-compatible

  $ configure modern
  $ setconfig paths.default=test:e1 ui.ssh=false

Prepare Repo:

  $ newremoterepo
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

  $ hg clone -U --shallow test:e1 --config remotefilelog.reponame=x --config clone.force-edenapi-clonedata=1 cloned1 -q
  $ cd cloned1

Commit and edit on top of B:

  $ LOG=dag::protocol=debug hg up $B -q
   DEBUG dag::protocol: resolve names [112478962961147124edd43549aedd1a335e44bf] remotely
   DEBUG dag::protocol: resolve ids [0] remotely
   DEBUG dag::protocol: resolve names [112478962961147124edd43549aedd1a335e44bf] remotely
  $ touch B1
  $ LOG=dag::protocol=debug hg commit -Am B1 B1
   DEBUG dag::protocol: resolve names [6450b32886cbb2753b70e8ee3ccc82db22a0aa84] remotely

  $ LOG=dag::protocol=debug hg metaedit -m B11
   DEBUG dag::protocol: resolve names [9eea6a043f2ec4b2de206be3b46bba3a2bb1c37b] remotely

(suboptimal: extra network fetches needed for checkout)
(suboptimal: extra network fetches needed for commit and amend)
