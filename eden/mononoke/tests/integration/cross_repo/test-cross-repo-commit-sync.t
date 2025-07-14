# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"
  $ setconfig ui.ignorerevnum=false

  $ init_two_small_one_large_repo
  A=e258521a78f8e12bee03bda35489701d887c41fd
  A=8ca76aa82bf928df58db99489fa17938e39774e4
  A=6ebc043d84761f4b77f73e4a2034cf5669bb6a54

-- get some bonsai hashes to avoid magic strings later
  $ FBSOURCE_MASTER_BONSAI=$(mononoke_admin bookmarks --repo-id 1 get master_bookmark)
  $ OVRSOURCE_MASTER_BONSAI=$(mononoke_admin bookmarks --repo-id 2 get master_bookmark)
  $ MEGAREPO_MERGE_BONSAI=$(mononoke_admin bookmarks --repo-id 0 get master_bookmark)

-- insert sync mapping entry
  $ add_synced_commit_mapping_entry 1 $FBSOURCE_MASTER_BONSAI 0 $MEGAREPO_MERGE_BONSAI TEST_VERSION_NAME
  $ add_synced_commit_mapping_entry 2 $OVRSOURCE_MASTER_BONSAI 0 $MEGAREPO_MERGE_BONSAI TEST_VERSION_NAME

-- start mononoke
  $ start_and_wait_for_mononoke_server
-- setup hg client repos
  $ cd "$TESTTMP"
  $ setconfig remotenames.selectivepulldefault=master_bookmark,somebook,fbsource/somebook
  $ hg clone -q mono:fbs-mon fbs-hg-cnt --noupdate
  $ hg clone -q mono:ovr-mon ovr-hg-cnt --noupdate
  $ hg clone -q mono:meg-mon meg-hg-cnt --noupdate

-- create an older version of fbsource_master, with a single simple change
  $ cd "$TESTTMP"/fbs-hg-cnt
  $ hg up -q master_bookmark
  $ createfile fbcode/fbcodefile2_fbsource
  $ createfile arvr/arvrfile2_fbsource
  $ hg -q ci -m "fbsource commit 2"
  $ hg push -r . --to master_bookmark -q

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
  $ hg push -r . --to master_bookmark -q

-- and a few more commits to master
  $ createfile fbcode/fbcodefile4_fbsource
  $ hg -q ci -m "fbsource commit 10"
  $ hg push -r . --to master_bookmark -q

-- Make a commit to non-master bookmark
  $ hg up -q 2
  $ createfile fbcode/non_master_file
  $ hg -q ci -m 'non-master commit'
  $ hg push -r . --to somebook --create -q

-- create unrelated bookmark in ovrsource that we'll intentionally skip
  $ cd "$TESTTMP/ovr-hg-cnt"
  $ hg up -q master_bookmark
  $ hg book forgotten_bookmark
  $ hg up -q forgotten_bookmark
  $ createfile unrelated_file
  $ hg -q ci -m "unrelated ovrsource commit"
  $ hg push -r . --to forgotten_bookmark --create -q
  $ export FORGOTTEN=$(hg whereami)

-- push from ovrsource
  $ hg up -q master_bookmark
  $ hg up -q master_bookmark
  $ createfile arvr/arvrfile2_ovrsource
  $ createfile fbcode/fbcodefile2_ovrsource
  $ createfile Research/researchfile2_ovrsource
  $ hg -q ci -m "ovrsource commit 2"
  $ hg push -r . --to master_bookmark -q

-- sync fbsource
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "INSERT INTO mutable_counters (repo_id, name, value) VALUES (0, 'xreposync_from_1', 1)";
  $ mononoke_x_repo_sync 1 0 tail --catch-up-once |& grep processing
  * processing log entry * (glob)
  * processing log entry * (glob)
  * processing log entry * (glob)
  * processing log entry * (glob)
  $ REPOIDLARGE=0 REPOIDSMALL=1 verify_wc $(mononoke_admin bookmarks --repo-id 0 get master_bookmark)
  $ REPOIDLARGE=0 REPOIDSMALL=1 verify_wc $(mononoke_admin bookmarks --repo-id 0 get fbsource/somebook)

-- sync ovrsource
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "INSERT INTO mutable_counters (repo_id, name, value) VALUES (0, 'xreposync_from_2', 2)"
  $ mononoke_x_repo_sync 2 0 tail --catch-up-once |& grep processing
  * processing log entry * (glob)
  $ REPOIDLARGE=0 REPOIDSMALL=2 verify_wc $(mononoke_admin bookmarks --repo-id 0 get master_bookmark)

-- one more push from fbsource to make sure resuming works
-- it also tests rewrite dates mode
  $ cd "$TESTTMP/fbs-hg-cnt"
  $ hg up master_bookmark -q
  $ createfile fbcode/resume
  $ hg -q ci -m "fbsource commit resume"
  $ hg push -r . --to master_bookmark -q
  $ mononoke_x_repo_sync 1 0  --pushrebase-rewrite-dates tail --catch-up-once |& grep processing
  * processing log entry * (glob)

  $ cd "$TESTTMP/meg-hg-cnt"
  $ hg pull -q
  $ hg up master_bookmark -q
  $ hg log -r ':' -T '{remotenames} {node} {date|shortdate} {desc}\n' | sort
   0c12c1c9c608c370527323d530d71f6eb195a8d7 1970-01-01 fbsource commit 9 (with removal of a preserved path)
   0e850883d3971709d8c651e78a6b67b016242f6d 1970-01-01 fbsource commit 7 (with copy of preserved path into moved path)
   1da063ce35d2cebfdfec3d833d66a2dfa90a9826 1970-01-01 fbsource commit 4 (with copy of preserved path into preserved path)
   59609a45b629de9dd9c4cd1b4ea287dd99310270 1970-01-01 fbsource commit 5 (with copy of moved path into moved path)
   6ebc043d84761f4b77f73e4a2034cf5669bb6a54 1970-01-01 megarepo commit 1
   8655945c2e0dbc72be804baa3a01c711b2de8808 1970-01-01 fbsource commit 6 (with copy of moved path into preserved path)
   9f5f9903501d62c1222dbd6ca29ecd998b777a7a 1970-01-01 fbsource commit 8 (with removal of a moved path)
   b44b30660223cd0fb02481332526b3dd65ad91e1 1970-01-01 fbsource commit 10
   b86bf896aebc7f5bb680047fc7a151e86787412a 1970-01-01 fbsource commit 2
   b9b15f2e79843b7430d15beaca47a61ae105a9f3 1970-01-01 ovrsource commit 2
   da3cabacad4dabc8a7fbef4dcb951374ff29bd79 1970-01-01 fbsource commit 3
  remote/fbsource/somebook 0d2d56e4a76414a34822d0630b4a09f6f44f1a87 1970-01-01 non-master commit
  remote/master_bookmark * 20*-*-* fbsource commit resume (glob)
  $ export FORGOTTEN_PARENT=$(hg log -T "{node}" -r b9b15f2e79843b7430d15beaca47a61ae105a9f3^)

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

fbsource should be fully in sync
  $ crossrepo_verify_bookmarks 0 1
  * all is well! (glob)

ovrsource has two problems
(master not matching is not a real problem though)
  $ quiet_grep bookmark -- crossrepo_verify_bookmarks 0 2 | sort
  [WARN] 'ovr-mon' has a bookmark master_bookmark but it points to a commit that has no equivalent in 'meg-mon'. If it's a shared bookmark (e.g. master) that might mean that it points to a commit from another repository
  [WARN] inconsistent value of forgotten_bookmark: 'ovr-mon' has 15ea1fb0b1b27b9d23175d1e7169e43d515c9aa06acf287cd992cbaf4908718e, but 'meg-mon' bookmark points to None

update-large-repo-bookmarks won't create commits by itself 
only the syncer can create the commit or they have to be imported some other way
  $ quiet_grep Missing -- crossrepo_verify_bookmarks 0 2 --update-large-repo-bookmarks 
  Error: Missing outcome for 15ea1fb0b1b27b9d23175d1e7169e43d515c9aa06acf287cd992cbaf4908718e from small repo
  [1]

but let's  say we synced that commit manually
  $ mononoke_admin megarepo manual-commit-sync --source-repo-id 2 --target-repo-id 0 --commit $FORGOTTEN --parents $FORGOTTEN_PARENT --mapping-version-name TEST_VERSION_NAME
  [INFO] using repo "ovr-mon" repoid RepositoryId(2)
  [INFO] using repo "meg-mon" repoid RepositoryId(0)
  [INFO] changeset resolved as: ChangesetId(Blake2(5e88c1738667e8b2f4ef54dd53d2ebfb42a6fc0997fd9c5d05cb3ae7e96d5330))
  [INFO] changeset resolved as: ChangesetId(Blake2(15ea1fb0b1b27b9d23175d1e7169e43d515c9aa06acf287cd992cbaf4908718e))
  [INFO] target cs id is Some(ChangesetId(Blake2(6f62563c84369fb14c82e97dc91a9f4bfe5ffa43db8f3cb7ea06b692c9f0d2e3)))

it still doesn't have any data derived
  $ mononoke_admin derived-data -R meg-mon exists -T changeset_info -i 6f62563c84369fb14c82e97dc91a9f4bfe5ffa43db8f3cb7ea06b692c9f0d2e3
  Not Derived: 6f62563c84369fb14c82e97dc91a9f4bfe5ffa43db8f3cb7ea06b692c9f0d2e3

and tried again, this time in dry run mode with limit 0 to ensure such command wouldn't do anything
  $ crossrepo_verify_bookmarks 0 2 --update-large-repo-bookmarks --no-bookmark-updates --limit 0|& grep bookmark | sort

it still doesn't have any data derived
  $ mononoke_admin derived-data -R meg-mon exists -T changeset_info -i 6f62563c84369fb14c82e97dc91a9f4bfe5ffa43db8f3cb7ea06b692c9f0d2e3
  Not Derived: 6f62563c84369fb14c82e97dc91a9f4bfe5ffa43db8f3cb7ea06b692c9f0d2e3

and tried again, this time in dry run mode with no limit
  $ quiet_grep bookmark -- crossrepo_verify_bookmarks 0 2 --update-large-repo-bookmarks --no-bookmark-updates --limit 2| sort
  [INFO] setting ovrsource/forgotten_bookmark 6f62563c84369fb14c82e97dc91a9f4bfe5ffa43db8f3cb7ea06b692c9f0d2e3
  [INFO] skipping master_bookmark because it's a common bookmark

and the data is derived
  $ mononoke_admin derived-data -R meg-mon exists -T changeset_info -i 6f62563c84369fb14c82e97dc91a9f4bfe5ffa43db8f3cb7ea06b692c9f0d2e3
  Derived: 6f62563c84369fb14c82e97dc91a9f4bfe5ffa43db8f3cb7ea06b692c9f0d2e3

and tried again
  $ quiet_grep bookmark -- crossrepo_verify_bookmarks 0 2 --update-large-repo-bookmarks | sort
  [INFO] setting ovrsource/forgotten_bookmark 6f62563c84369fb14c82e97dc91a9f4bfe5ffa43db8f3cb7ea06b692c9f0d2e3
  [INFO] skipping master_bookmark because it's a common bookmark

now the verification shouldn't return that error
  $ crossrepo_verify_bookmarks 0 2
  * 'ovr-mon' has a bookmark master_bookmark but it points to a commit that has no equivalent in 'meg-mon'. If it's a shared bookmark (e.g. master) that might mean that it points to a commit from another repository (glob)
  * found 1 inconsistencies (glob)
  [1]
