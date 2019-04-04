  $ enable amend absorb
  $ setconfig extensions.extralog=$TESTDIR/extralog.py
  $ setconfig extralog.events=commit_info extralog.keywords=true

  $ newrepo

  $ echo base > base
  $ hg commit -Am base
  adding base
  commit_info:  (author=test node=d20a80d4def38df63a4b330b7fb688f3d4cae1e3)

  $ echo 1 > 1
  $ hg commit -Am 1
  adding 1
  commit_info:  (author=test node=f0161ad23099c690115006c21e96f780f5d740b6)
  $ echo 1b > 1
  $ hg amend -m 1b
  commit_info:  (author=test node=edbfe685c913f3cec015588dbc0f1e03f5146d80)
