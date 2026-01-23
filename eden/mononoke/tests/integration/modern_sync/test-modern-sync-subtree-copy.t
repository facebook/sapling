# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ configure modern
  $ setconfig subtree.use-prod-subtree-key=True
  $ setconfig subtree.min-path-depth=1
  $ setconfig remotenames.selectivepulldefault=master_bookmark

  $ cat >> "$ACL_FILE" << ACLS
  > {
  >   "repos": {
  >     "orig": {
  >       "actions": {
  >         "read": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"],
  >         "write": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"],
  >         "bypass_readonly": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"]
  >       }
  >     },
  >     "dest": {
  >       "actions": {
  >         "read": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA","SERVICE_IDENTITY:server", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"],
  >         "write": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA","SERVICE_IDENTITY:server", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"],
  >          "bypass_readonly": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA","SERVICE_IDENTITY:server", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"]
  >       }
  >     }
  >   },
  >   "tiers": {
  >     "mirror_commit_upload": {
  >       "actions": {
  >         "mirror_upload": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA","SERVICE_IDENTITY:server", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"]
  >       }
  >     }
  >   }
  > }
  > ACLS

  $ REPOID=0 REPONAME=orig ACL_NAME=orig setup_common_config
  $ REPOID=1 REPONAME=dest ACL_NAME=dest setup_common_config

  $ cd $TESTTMP

Create a git repo that we will import later on
  $ git init -q gitrepo
  $ cd gitrepo
  $ git config core.autocrlf false
  $ echo 1 > alpha
  $ git add alpha
  $ git commit -q -m alpha
  $ mkdir dir1
  $ echo 2 > dir1/beta
  $ git add dir1/beta
  $ git commit -q -m beta
  $ export GIT_URL=git+file://$TESTTMP/gitrepo

  $ cd $TESTTMP

  $ testtool_drawdag -R orig --derive-all --print-hg-hashes <<EOF
  > A-B-C
  > # bookmark: C master_bookmark
  > EOF
  A=20ca2a4749a439b459125ef0f6a4f26e88ee7538
  B=80521a640a0c8f51dcc128c2658b224d595840ac
  C=d3b399ca8757acdb81c3681b052eb978db6768d8

  $ start_and_wait_for_mononoke_server
  $ hg clone -q mono:orig orig
  $ cd orig
  $ hg up -q $C

Create a graph with a subtree copy
  $ mkdir -p dir1
  $ echo d > dir1/d
  $ hg commit -Aqm 'add d'
  $ D=$(hg whereami)
  $ echo e > dir1/e
  $ hg commit -Aqm 'add e'
  $ E=$(hg whereami)
  $ hg push -r . --to master_bookmark -q
  $ hg subtree copy -r $D --from-path dir1 --to-path dir2
  copying dir1 to dir2
  $ hg push -r . --to master_bookmark -q
  $ F=$(hg whereami)
  $ hg up -q $B
  $ hg subtree copy -r $F --from-path dir1 --to-path dir3
  copying dir1 to dir3
  $ echo g > dir3/g
  $ hg commit -Aqm 'add g'
  $ G=$(hg whereami)
  $ hg push -r . --to other_bookmark --create
  pushing rev 27d3ae184ccb to destination mono:orig bookmark other_bookmark
  searching for changes
  exporting bookmark other_bookmark
  $ hg pull -B other_bookmark
  pulling from mono:orig
  $ hg up -q $F
  $ echo h > dir2/h
  $ hg commit -Aqm 'add h'
  $ hg subtree merge -r $G --from-path dir3 --to-path dir2
  searching for merge base ...
  found the last subtree copy commit 76b1ed688333
  merge base: 080e9f075a1f
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg commit -Aqm 'subtree merge'
  $ hg push --to master_bookmark -q
  $ hg subtree import --url $GIT_URL --rev master_bookmark --to-path bar -m "import gitrepo to bar"
  creating git repo at $TESTTMP/cachepath/gitrepos/* (glob)
  From file://$TESTTMP/gitrepo
   * [new ref]         4c67869f7948534db7e9f5ff08d35569569849d9 -> remote/master_bookmark
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  copying / to bar
  $ I=$(hg whereami)
  $ hg push --to master_bookmark -q
  $ hg log -G -T '{node} {desc}\n' -r "all()"
  o  27d3ae184ccb321e9fc87eb2cace5c8e5370d22a add g
  │
  o  76b1ed688333a039be662fb7c759fc476e9018a5 Subtree copy from 080e9f075a1f3858b670b0b1d6f7f9c981019677
  │  - Copied path dir1 to dir3
  │ @  * import gitrepo to bar (glob)
  │ │
  │ │  Subtree import from git+file://$TESTTMP/gitrepo at 4c67869f7948534db7e9f5ff08d35569569849d9
  │ │  - Imported path root directory to bar
  │ o  fd1e9fb190a897eb437bf949afd77baed7657847 subtree merge
  │ │
  │ │  Subtree merge from 27d3ae184ccb321e9fc87eb2cace5c8e5370d22a
  │ │  - Merged path dir3 to dir2
  │ o  81534e33fb7b22f93e957167d2751cd9e65665af add h
  │ │
  │ o  080e9f075a1f3858b670b0b1d6f7f9c981019677 Subtree copy from 16f25db8a3333adf318a5d55b62eb3fb8c5d2edc
  │ │  - Copied path dir1 to dir2
  │ o  f4b7c40ac3ff16330148cd0ad25028dc89faadb2 add e
  │ │
  │ o  16f25db8a3333adf318a5d55b62eb3fb8c5d2edc add d
  │ │
  │ o  d3b399ca8757acdb81c3681b052eb978db6768d8 C
  ├─╯
  o  80521a640a0c8f51dcc128c2658b224d595840ac B
  │
  o  20ca2a4749a439b459125ef0f6a4f26e88ee7538 A
  
Sync all bookmarks moves
  $ quiet mononoke_modern_sync "" sync-once orig dest --start-id 0

Clone and verify the destination repo
  $ cd ..
  $ hg clone -q mono:dest dest
  $ cd dest
  $ hg log -G -T '{node} {desc}\n' -r "all()"
  @  * import gitrepo to bar (glob)
  │
  │  Subtree import from git+file://$TESTTMP/gitrepo at 4c67869f7948534db7e9f5ff08d35569569849d9
  │  - Imported path root directory to bar
  o  fd1e9fb190a897eb437bf949afd77baed7657847 subtree merge
  │
  │  Subtree merge from 27d3ae184ccb321e9fc87eb2cace5c8e5370d22a
  │  - Merged path dir3 to dir2
  o  81534e33fb7b22f93e957167d2751cd9e65665af add h
  │
  o  080e9f075a1f3858b670b0b1d6f7f9c981019677 Subtree copy from 16f25db8a3333adf318a5d55b62eb3fb8c5d2edc
  │  - Copied path dir1 to dir2
  o  f4b7c40ac3ff16330148cd0ad25028dc89faadb2 add e
  │
  o  16f25db8a3333adf318a5d55b62eb3fb8c5d2edc add d
  │
  o  d3b399ca8757acdb81c3681b052eb978db6768d8 C
  │
  o  80521a640a0c8f51dcc128c2658b224d595840ac B
  │
  o  20ca2a4749a439b459125ef0f6a4f26e88ee7538 A
  
  $ hg subtree inspect $I
  {
    "imports": [
      {
        "version": 1,
        "url": "git+file://$TESTTMP/gitrepo",
        "from_commit": "4c67869f7948534db7e9f5ff08d35569569849d9",
        "from_path": "",
        "to_path": "bar"
      }
    ]
  }

Commit G got synced due to being a subtree source, and so is avablable even though it's not an ancestor of master.
  $ hg pull -r $G
  pulling from mono:dest
  searching for changes
  $ hg log -G -T '{node} {desc}\n' -r "all()"
  o  27d3ae184ccb321e9fc87eb2cace5c8e5370d22a add g
  │
  o  76b1ed688333a039be662fb7c759fc476e9018a5 Subtree copy from 080e9f075a1f3858b670b0b1d6f7f9c981019677
  │  - Copied path dir1 to dir3
  │ @  * import gitrepo to bar (glob)
  │ │
  │ │  Subtree import from git+file://$TESTTMP/gitrepo at 4c67869f7948534db7e9f5ff08d35569569849d9
  │ │  - Imported path root directory to bar
  │ o  fd1e9fb190a897eb437bf949afd77baed7657847 subtree merge
  │ │
  │ │  Subtree merge from 27d3ae184ccb321e9fc87eb2cace5c8e5370d22a
  │ │  - Merged path dir3 to dir2
  │ o  81534e33fb7b22f93e957167d2751cd9e65665af add h
  │ │
  │ o  080e9f075a1f3858b670b0b1d6f7f9c981019677 Subtree copy from 16f25db8a3333adf318a5d55b62eb3fb8c5d2edc
  │ │  - Copied path dir1 to dir2
  │ o  f4b7c40ac3ff16330148cd0ad25028dc89faadb2 add e
  │ │
  │ o  16f25db8a3333adf318a5d55b62eb3fb8c5d2edc add d
  │ │
  │ o  d3b399ca8757acdb81c3681b052eb978db6768d8 C
  ├─╯
  o  80521a640a0c8f51dcc128c2658b224d595840ac B
  │
  o  20ca2a4749a439b459125ef0f6a4f26e88ee7538 A
  
