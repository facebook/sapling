
#require no-eden


  $ eagerepo
  $ enable amend absorb rebase
  $ setconfig extensions.extralog=$TESTDIR/extralog.py
  $ setconfig extralog.events=commit_info extralog.keywords=true
  $ setconfig extensions.stableindentifiers=$TESTDIR/stableidentifiers.py

  $ newrepo

Commit and Amend
  $ echo base > base
  $ hg commit -Am base
  adding base
  commit_info (author=test checkoutidentifier= node=d20a80d4def38df63a4b330b7fb688f3d4cae1e3 repo=None)

  $ hg debugcheckoutidentifier
  0000000000000000
  $ echo 1 > 1
  $ hg add 1
  $ hg debugcheckoutidentifier
  0000000000000000
  $ hg commit -m 1
  commit_info (author=test checkoutidentifier=0000000000000000 node=f0161ad23099c690115006c21e96f780f5d740b6 repo=None)

  $ hg debugcheckoutidentifier
  0000000000000001
  $ echo 1b > 1
  $ hg amend -m 1b
  commit_info (author=test checkoutidentifier=0000000000000001 mutation=amend node=edbfe685c913f3cec015588dbc0f1e03f5146d80 predecessors=f0161ad23099c690115006c21e96f780f5d740b6 repo=None)

  $ hg debugcheckoutidentifier
  0000000000000002
  $ echo 2 > 2
  $ hg commit -Am 2
  adding 2
  commit_info (author=test checkoutidentifier=0000000000000002 node=155c3fe008ceed8a313cbb9358999d850a57a06f repo=None)

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
  commit_info (author=test checkoutidentifier=0000000000000003 mutation=absorb node=ee4c006896a84aefb2a11265c3d111fa29c5303a predecessors=edbfe685c913f3cec015588dbc0f1e03f5146d80 repo=None)
  commit_info (author=test checkoutidentifier=0000000000000003 mutation=absorb node=4db57f75ff6289054315f0e20d85703b6122d922 predecessors=155c3fe008ceed8a313cbb9358999d850a57a06f repo=None)
  2 of 2 chunks applied

Rebase with conflict resolution
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [ee4c00] 1b
  $ hg debugcheckoutidentifier
  0000000000000005
  $ echo 2z > 2
  $ hg commit -Am 2z
  adding 2
  commit_info (author=test checkoutidentifier=0000000000000005 node=3affbc31f750be084dd0ff1123cb35f51949a4bf repo=None)
  $ hg debugcheckoutidentifier
  0000000000000006
  $ echo 3 > 3
  $ hg commit -Am 3
  adding 3
  commit_info (author=test checkoutidentifier=0000000000000006 node=c889456c382b425f6cc387cccb7b42d176e7fe4f repo=None)
  $ hg debugcheckoutidentifier
  0000000000000007
  $ hg rebase -s 'desc(2z)' -d 4db57f75ff6289054315f0e20d85703b6122d922
  rebasing 3affbc31f750 "2z"
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
  rebasing 3affbc31f750 "2z"
  commit_info (author=test checkoutidentifier=0000000000000008 mutation=rebase node=bf042268a5395c9182ee4baa4385dc5de61ba908 predecessors=3affbc31f750be084dd0ff1123cb35f51949a4bf repo=None)
  rebasing c889456c382b "3"
  commit_info (author=test checkoutidentifier=0000000000000009 mutation=rebase node=f6174ca30f1c6747302f2b50c21c9ae1abcc3325 predecessors=c889456c382b425f6cc387cccb7b42d176e7fe4f repo=None)
  $ hg debugcheckoutidentifier
  0000000000000010

Fold has no checkoutidentifier, but does log other commit info
  $ hg fold --from ".~2"
  commit_info (author=test mutation=fold node=659841fb007feadfad44964c25f67a36b46ac26b predecessors=4db57f75ff6289054315f0e20d85703b6122d922 bf042268a5395c9182ee4baa4385dc5de61ba908 f6174ca30f1c6747302f2b50c21c9ae1abcc3325 repo=None)
  3 changesets folded
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg debugcheckoutidentifier
  0000000000000011
