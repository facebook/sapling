# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"
  $ setconfig ui.ignorerevnum=false

  $ init_two_small_one_large_repo

-- get some bonsai hashes to avoid magic strings later
  $ FBSOURCE_MASTER_BONSAI=$(get_bonsai_bookmark 1 master_bookmark)
  $ OVRSOURCE_MASTER_BONSAI=$(get_bonsai_bookmark 2 master_bookmark)
  $ MEGAREPO_MERGE_BONSAI=$(get_bonsai_bookmark 0 master_bookmark)

-- insert sync mapping entry
  $ add_synced_commit_mapping_entry 1 $FBSOURCE_MASTER_BONSAI 0 $MEGAREPO_MERGE_BONSAI TEST_VERSION_NAME
  $ add_synced_commit_mapping_entry 2 $OVRSOURCE_MASTER_BONSAI 0 $MEGAREPO_MERGE_BONSAI TEST_VERSION_NAME

-- setup hg client repos
  $ cd "$TESTTMP"
  $ hgclone_treemanifest ssh://user@dummy/fbs-hg-srv fbs-hg-cnt --noupdate
  $ hgclone_treemanifest ssh://user@dummy/ovr-hg-srv ovr-hg-cnt --noupdate
  $ hgclone_treemanifest ssh://user@dummy/meg-hg-srv meg-hg-cnt --noupdate

-- start mononoke
  $ start_and_wait_for_mononoke_server
-- create an older version of fbsource_master, with a single simple change
  $ cd "$TESTTMP"/fbs-hg-cnt
  $ REPONAME=fbs-mon hgmn up -q master_bookmark
  $ createfile fbcode/fbcodefile2_fbsource
  $ createfile arvr/arvrfile2_fbsource
  $ hg -q ci -m "fbsource commit 2"
  $ REPONAME=fbs-mon hgmn push -r . --to master_bookmark -q

-- create newer version fbsource_master_newer with more complex changes and more commits
  $ createfile fbcode/fbcodefile3_fbsource
  $ hg -q ci -m "fbsource commit 3"
  $ hg -q cp fbcode/fbcodefile3_fbsource fbcode/fbcodefile3_copy_fbsource
  $ hg -q ci -m "fbsource commit 4 (with copy of preserved path into preserved path)"
  $ hg -q cp arvr/arvrfile_fbsource arvr/arvrfile_copy_fbsource
  $ hg -q ci -m "fbsource commit 5 (with copy of moved path into moved path)"
  $ hg -q cp arvr/arvrfile_fbsource fbcode/arvrfile_copy_fbsource
  $ hg -q ci -m "fbsource commit 6 (with copy of moved path into preserved path)"
  $ hg -q cp fbcode/fbcodefile_fbsource arvr/fbcodefile_fbsource
  $ hg -q ci -m "fbsource commit 7 (with copy of preserved path into moved path)"
  $ hg -q rm arvr/fbcodefile_fbsource
  $ hg -q ci -m "fbsource commit 8 (with removal of a moved path)"
  $ hg -q rm fbcode/arvrfile_copy_fbsource
  $ hg -q ci -m "fbsource commit 9 (with removal of a preserved path)"
  $ REPONAME=fbs-mon hgmn push -r . --to master_bookmark -q

-- and a few more commits to master
  $ createfile fbcode/fbcodefile4_fbsource
  $ hg -q ci -m "fbsource commit 10"
  $ REPONAME=fbs-mon hgmn push -r . --to master_bookmark -q

-- Make a commit to non-master bookmark
  $ REPONAME=fbs-mon hgmn up -q 2
  $ createfile fbcode/non_master_file
  $ hg -q ci -m 'non-master commit'
  $ REPONAME=fbs-mon hgmn push -r . --to somebook --create -q

-- push from ovrsource
  $ cd "$TESTTMP/ovr-hg-cnt"
  $ REPONAME=ovr-mon hgmn up -q master_bookmark
  $ createfile arvr/arvrfile2_ovrsource
  $ createfile fbcode/fbcodefile2_ovrsource
  $ createfile Research/researchfile2_ovrsource
  $ hg -q ci -m "ovrsource commit 2"
  $ REPONAME=ovr-mon hgmn push -r . --to master_bookmark -q

-- sync fbsource
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "INSERT INTO mutable_counters (repo_id, name, value) VALUES (0, 'xreposync_from_1', 1)";
  $ mononoke_x_repo_sync 1 0 tail --catch-up-once |& grep processing
  * processing log entry * (glob)
  * processing log entry * (glob)
  * processing log entry * (glob)
  * processing log entry * (glob)
  $ REPOIDLARGE=0 REPOIDSMALL=1 verify_wc master_bookmark
  $ REPOIDLARGE=0 REPOIDSMALL=1 verify_wc fbsource/somebook

-- sync ovrsource
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "INSERT INTO mutable_counters (repo_id, name, value) VALUES (0, 'xreposync_from_2', 0)";
  $ mononoke_x_repo_sync 2 0 tail --catch-up-once |& grep processing
  * processing log entry * (glob)
  * processing log entry * (glob)
  $ REPOIDLARGE=0 REPOIDSMALL=2 verify_wc master_bookmark

-- one more push from fbsource to make sure resuming works
  $ cd "$TESTTMP/fbs-hg-cnt"
  $ REPONAME=fbs-mon hgmn up master_bookmark -q
  $ createfile fbcode/resume
  $ hg -q ci -m "fbsource commit resume"
  $ REPONAME=fbs-mon hgmn push -r . --to master_bookmark -q
  $ mononoke_x_repo_sync 1 0 tail --catch-up-once |& grep processing
  * processing log entry * (glob)
  $ flush_mononoke_bookmarks

  $ flush_mononoke_bookmarks

  $ cd "$TESTTMP/meg-hg-cnt"
  $ REPONAME=meg-mon hgmn pull -q
  $ REPONAME=meg-mon hgmn up master_bookmark -q
  $ hg log -r ':' -T '{remotenames} {node} {desc}\n' | sort
   094d073f382b989af68f25267225b2e44ccf43c3 fbsource commit 8 (with removal of a moved path)
   3b1e6b17c7fed77f139c75a5a241268d41d584f8 fbsource commit 4 (with copy of preserved path into preserved path)
   4fda77caf85f3399fcb637a871a5e621c6796a6d fbsource commit 9 (with removal of a preserved path)
   5e3b9ed38bf2153213dd8dd8841a7772d1e27074 fbsource commit 7 (with copy of preserved path into moved path)
   763de6470427ef760e578a8dd87ecaae759cf9d1 fbsource commit 6 (with copy of moved path into preserved path)
   7c7fb16d4ed18604106835e59fee72344603afa7 fbsource commit 5 (with copy of moved path into moved path)
   83da1de25a2a199f98ede29c06bc22e54943cc47 megarepo commit 1
   99c848e3f5ff3ab7746fb71816748e2ba0d7da36 fbsource commit 2
   b0474d400edddcabef0a27ead293a6b99ae59490 ovrsource commit 2
   b06de5da9e40e0da6eda1f7b5c891711106d707b fbsource commit 3
   e0cb430152c2dcc47b93a516344e3814ece60d4b fbsource commit 10
  default/fbsource/somebook d692e38644b938ccccc4192bd2f507955f3888c5 non-master commit
  default/master_bookmark 8d01dd2e0e909e21d3131b7929787db006de999e fbsource commit resume

-- Validate the synced entries
  $ REPOIDLARGE=0 validate_commit_sync 10 |& grep "Validated entry"
  * Validated entry: Entry 10 (1/1) (glob)

  $ REPOIDLARGE=0 validate_commit_sync 11 |& grep "Validated entry"
  * Validated entry: Entry 11 (1/1) (glob)

  $ REPOIDLARGE=0 validate_commit_sync 12 |& grep "Validated entry"
  * Validated entry: Entry 12 (1/1) (glob)

  $ REPOIDLARGE=0 validate_commit_sync 13 |& grep "Validated entry"
  * Validated entry: Entry 13 (1/1) (glob)

Query synced commit mapping, check that automatically inserted mappings have version_name
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" << EOQ
  > SELECT DISTINCT sync_map_version_name
  > FROM synced_commit_mapping
  > WHERE small_bcs_id NOT IN (X'$FBSOURCE_MASTER_BONSAI', X'$OVRSOURCE_MASTER_BONSAI');
  > EOQ
  TEST_VERSION_NAME
