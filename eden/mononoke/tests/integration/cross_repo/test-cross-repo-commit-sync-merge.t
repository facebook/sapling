# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ REPOTYPE="blob_files"
  $ FBS_REPO="fbs-mon"
  $ MEG_REPO="meg-mon"
  $ REPOID=0 REPONAME=$MEG_REPO setup_common_config $REPOTYPE
  $ REPOID=1 REPONAME=$FBS_REPO setup_common_config $REPOTYPE
  $ setup_commitsyncmap
  $ setup_configerator_configs
  $ ls $TESTTMP/monsql/sqlite_dbs
  ls: cannot access *: No such file or directory (glob)
  [2]


setup hg server repos
  $ function createfile { mkdir -p "$(dirname  $1)" && echo "$1" > "$1" && hg add -q "$1"; }

  $ cd $TESTTMP
  $ testtool_drawdag --print-hg-hashes -R $FBS_REPO --derive-all --no-default-files <<EOF
  > A-B
  > C
  > # message: A "fbsource commit 1"
  > # modify: A "fbcode/fbcodefile_fbsource" "fbcode/fbcodefile_fbsource"
  > # message: B "fbsource commit 2"
  > # modify: B "arvr/arvrfile_fbsource" "arvr/arvrfile_fbsource"
  > # modify: B "otherfile_fbsource" "otherfile_fbsource"
  > # modify: B "b" "file_content"
  > # bookmark: B fbsource_c1
  > # message: C "to merge"
  > # modify: C "arvr/tomerge" "arvr/tomerge"
  > # bookmark: C to_merge
  > EOF
  A=ff190b38dcc48bacba77a5183c4cce7006852b85
  B=45cdd1a0553a8972b9ba6344ea4ac396ce88a655
  C=5b6a09b38022b07a87e13b38aaa28197b27495c3


  $ testtool_drawdag --print-hg-hashes -R $MEG_REPO --derive-all --no-default-files <<EOF
  > A
  > # message: A "megarepo commit 1"
  > # modify: A "fbcode/fbcodefile_fbsource" "fbcode/fbcodefile_fbsource"
  > # modify: A ".fbsource-rest/arvr/arvrfile_fbsource" ".fbsource-rest/arvr/arvrfile_fbsource"
  > # modify: A "otherfile_fbsource" "otherfile_fbsource"
  > # modify: A ".ovrsource-rest/fbcode/fbcodefile_ovrsource" ".ovrsource-rest/fbcode/fbcodefile_ovrsource"
  > # modify: A "arvr/arvrfile_ovrsource" "arvr/arvrfile_ovrsource"
  > # modify: A "arvr-legacy/Research/researchfile_ovrsource" "arvr-legacy/Research/researchfile_ovrsource"
  > # modify: A "arvr-legacy/otherfile_ovrsource" "arvr-legacy/otherfile_ovrsource"
  > # bookmark: A master_bookmark
  > EOF
  A=abe2e94615b738949a81cbb940fbca24717cf22c

  $ start_and_wait_for_mononoke_server


-- Create merge commit
  $ hg clone "mono:$FBS_REPO" $FBS_REPO
  fetching lazy changelog
  populating main commit graph
  updating to tip
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd $FBS_REPO
  $ hg pull -q -B fbsource_c1
  $ hg checkout -q fbsource_c1
  $ hg up -q .
  $ hg merge -q "$C"
  $ hg -q ci -m 'merge_commit'
  $ hg push -q --to fbsource_master --create

-- Clone megarepo
  $ cd $TESTTMP
  $ hg clone "mono:$MEG_REPO" $MEG_REPO
  fetching lazy changelog
  populating main commit graph
  updating to tip
  7 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd $MEG_REPO
  $ hg log
  commit:      abe2e94615b7
  bookmark:    remote/master_bookmark
  hoistedname: master_bookmark
  user:        author
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     megarepo commit 1
  
get some bonsai hashes to avoid magic strings later
  $ FBSOURCE_C1_BONSAI=$(mononoke_admin bookmarks --repo-id 1 get fbsource_c1)
  $ FBSOURCE_MASTER_BONSAI=$(mononoke_admin bookmarks --repo-id 1 get fbsource_master)
  $ MEGAREPO_MERGE_BONSAI=$(mononoke_admin bookmarks --repo-id 0 get master_bookmark)

  $ cd $TESTTMP

start mononoke server
  $ start_and_wait_for_mononoke_server
insert sync mapping entry
  $ add_synced_commit_mapping_entry 1 $FBSOURCE_C1_BONSAI 0 $MEGAREPO_MERGE_BONSAI TEST_VERSION_NAME

run the sync again
  $ mononoke_x_repo_sync 1 0 once --target-bookmark master_bookmark -B fbsource_master |& grep -v "using repo"
  [INFO] Starting session with id * (glob)
  [INFO] Starting up X Repo Sync from small repo fbs-mon to large repo meg-mon
  [INFO] Syncing 1 commits and all of their unsynced ancestors
  [INFO] Checking if 185ab836cc4952b370393e74f342c5ab3dd6f56cadcb42a58ef48e01823076fd is already synced 1->0
  [INFO] 2 unsynced ancestors of 185ab836cc4952b370393e74f342c5ab3dd6f56cadcb42a58ef48e01823076fd
  [INFO] syncing 9d41183ab69f7a8ccb85011c35ecbd7329d76bdc96e765459176bf4cc3fe1683
  [INFO] changeset 9d41183ab69f7a8ccb85011c35ecbd7329d76bdc96e765459176bf4cc3fe1683 synced as fc7956caaa6324fff247c4990221e10cb234309e3bc571cff00739f7c08adcbd in * (glob)
  [INFO] syncing 185ab836cc4952b370393e74f342c5ab3dd6f56cadcb42a58ef48e01823076fd via pushrebase for master_bookmark
  [INFO] changeset 185ab836cc4952b370393e74f342c5ab3dd6f56cadcb42a58ef48e01823076fd synced as 5d1ca261178e0ebb020ed452430815db8c00eaee93fe7cc7ccf8a8cadb3c7abe in * (glob)
  [INFO] successful sync
  [INFO] X Repo Sync execution finished from small repo fbs-mon to large repo meg-mon
  $ flush_mononoke_bookmarks

check that the changes are synced
  $ cd $MEG_REPO
  $ hg -q pull
  $ hg -q status --change master_bookmark 2>/dev/null
  A .fbsource-rest/arvr/tomerge
  $ hg status --change 0f290f150a2b
  A .fbsource-rest/arvr/tomerge
  $ hg log -G
  o    commit:      597e4f00f62e
  ├─╮  bookmark:    remote/master_bookmark
  │ │  hoistedname: master_bookmark
  │ │  user:        test
  │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │  summary:     merge_commit
  │ │
  │ o  commit:      0f290f150a2b
  │    user:        author
  │    date:        Thu Jan 01 00:00:00 1970 +0000
  │    summary:     to merge
  │
  @  commit:      abe2e94615b7
     user:        author
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     megarepo commit 1
  
