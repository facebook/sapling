#chg-compatible

  $ enable amend absorb rebase
  $ setconfig extensions.extralog=$TESTDIR/extralog.py
  $ setconfig extralog.events=commit_info extralog.keywords=true
  $ setconfig extensions.stableindentifiers=$TESTDIR/stableidentifiers.py

  $ newrepo

Commit and Amend
  $ echo base > base
  $ hg commit -Am base
  adding base
  commit_info (author=test checkoutidentifier= node=d20a80d4def38df63a4b330b7fb688f3d4cae1e3)

  $ hg debugcheckoutidentifier
  0000000000000000
  $ echo 1 > 1
  $ hg add 1
  $ hg debugcheckoutidentifier
  0000000000000000
  $ hg commit -m 1
  commit_info (author=test checkoutidentifier=0000000000000000 node=f0161ad23099c690115006c21e96f780f5d740b6)

  $ hg debugcheckoutidentifier
  0000000000000001
  $ echo 1b > 1
  $ hg amend -m 1b
  commit_info (author=test checkoutidentifier=0000000000000001 mutation=amend node=edbfe685c913f3cec015588dbc0f1e03f5146d80 predecessors=f0161ad23099c690115006c21e96f780f5d740b6)

  $ hg debugcheckoutidentifier
  0000000000000002
  $ echo 2 > 2
  $ hg commit -Am 2
  adding 2
  commit_info (author=test checkoutidentifier=0000000000000002 node=155c3fe008ceed8a313cbb9358999d850a57a06f)

Absorb
  $ hg debugcheckoutidentifier
  0000000000000003
  $ echo 2b > 2
  $ echo 1c > 1
  $ hg absorb -a
  showing changes for 1
          @@ -0,1 +0,1 @@
  edbfe68 -1b
  edbfe68 +1c
  showing changes for 2
          @@ -0,1 +0,1 @@
  155c3fe -2
  155c3fe +2b
  
  2 changesets affected
  155c3fe 2
  edbfe68 1b
  commit_info (author=test checkoutidentifier=0000000000000003 node=f84ddfee68927d4ebfe4344520adb71ccb173c4f)
  commit_info (author=test checkoutidentifier=0000000000000003 node=e911dd548c90906d9f6733aa1612274865a7dfd2)
  2 of 2 chunks applied

Rebase with conflict resolution
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [f84ddf] 1b
  $ hg debugcheckoutidentifier
  0000000000000005
  $ echo 2z > 2
  $ hg commit -Am 2z
  adding 2
  commit_info (author=test checkoutidentifier=0000000000000005 node=78930e916793ff11b38f4f89f92221c180f922a3)
  $ hg debugcheckoutidentifier
  0000000000000006
  $ echo 3 > 3
  $ hg commit -Am 3
  adding 3
  commit_info (author=test checkoutidentifier=0000000000000006 node=27fd2733660ce0233ef4603cebe6328681aa598d)
  $ hg debugcheckoutidentifier
  0000000000000007
  $ hg rebase -s 6 -d 5
  rebasing 78930e916793 "2z"
  merging 2
  warning: 1 conflicts while merging 2! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg debugcheckoutidentifier
  0000000000000008
  $ echo 2merged > 2
  $ hg resolve --mark 2
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg debugcheckoutidentifier
  0000000000000008
  $ hg rebase --continue
  rebasing 78930e916793 "2z"
  commit_info (author=test checkoutidentifier=0000000000000008 mutation=rebase node=2a9f3f40eebf9d189f51eeba40f6d45935255c3e predecessors=78930e916793ff11b38f4f89f92221c180f922a3)
  rebasing 27fd2733660c "3"
  commit_info (author=test checkoutidentifier=0000000000000009 mutation=rebase node=b42c49c8c650d6040d4a4003a30c82e1cde21c50 predecessors=27fd2733660ce0233ef4603cebe6328681aa598d)
  $ hg debugcheckoutidentifier
  0000000000000010

Fold has no checkoutidentifier, but does log other commit info
  $ hg fold --from ".~2"
  commit_info (author=test mutation=fold node=39938ad744a3c4695743296607b5786b8e1437c6 predecessors=e911dd548c90906d9f6733aa1612274865a7dfd2 2a9f3f40eebf9d189f51eeba40f6d45935255c3e b42c49c8c650d6040d4a4003a30c82e1cde21c50)
  3 changesets folded
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg debugcheckoutidentifier
  0000000000000011
