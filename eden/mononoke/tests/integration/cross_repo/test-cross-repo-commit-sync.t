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
  $ FBSOURCE_MASTER_BONSAI=$(mononoke_newadmin bookmarks --repo-id 1 get master_bookmark)
  $ OVRSOURCE_MASTER_BONSAI=$(mononoke_newadmin bookmarks --repo-id 2 get master_bookmark)
  $ MEGAREPO_MERGE_BONSAI=$(mononoke_newadmin bookmarks --repo-id 0 get master_bookmark)

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

-- create unrelated bookmark in ovrsource that we'll intentionally skip
  $ cd "$TESTTMP/ovr-hg-cnt"
  $ hg up -q master_bookmark
  $ hg book forgotten_bookmark
  $ hg up -q forgotten_bookmark
  $ createfile unrelated_file
  $ hg -q ci -m "unrelated ovrsource commit"
  $ REPONAME=ovr-mon hgmn push -r . --to forgotten_bookmark --create -q
  $ export FORGOTTEN=$(hg whereami)

-- push from ovrsource
  $ hg up -q master_bookmark
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
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "INSERT INTO mutable_counters (repo_id, name, value) VALUES (0, 'xreposync_from_2', 2)"
  $ mononoke_x_repo_sync 2 0 tail --catch-up-once |& grep processing
  * processing log entry * (glob)
  $ REPOIDLARGE=0 REPOIDSMALL=2 verify_wc master_bookmark

-- one more push from fbsource to make sure resuming works
-- it also tests rewrite dates mode
  $ cd "$TESTTMP/fbs-hg-cnt"
  $ REPONAME=fbs-mon hgmn up master_bookmark -q
  $ createfile fbcode/resume
  $ hg -q ci -m "fbsource commit resume"
  $ REPONAME=fbs-mon hgmn push -r . --to master_bookmark -q
  $ mononoke_x_repo_sync 1 0  --pushrebase-rewrite-dates tail --catch-up-once |& grep processing
  * processing log entry * (glob)
  $ flush_mononoke_bookmarks

  $ flush_mononoke_bookmarks

  $ cd "$TESTTMP/meg-hg-cnt"
  $ REPONAME=meg-mon hgmn pull -q
  $ REPONAME=meg-mon hgmn up master_bookmark -q
  $ hg log -r ':' -T '{remotenames} {node} {date|shortdate} {desc}\n' | sort
   094d073f382b989af68f25267225b2e44ccf43c3 1970-01-01 fbsource commit 8 (with removal of a moved path)
   3b1e6b17c7fed77f139c75a5a241268d41d584f8 1970-01-01 fbsource commit 4 (with copy of preserved path into preserved path)
   4fda77caf85f3399fcb637a871a5e621c6796a6d 1970-01-01 fbsource commit 9 (with removal of a preserved path)
   5e3b9ed38bf2153213dd8dd8841a7772d1e27074 1970-01-01 fbsource commit 7 (with copy of preserved path into moved path)
   763de6470427ef760e578a8dd87ecaae759cf9d1 1970-01-01 fbsource commit 6 (with copy of moved path into preserved path)
   7c7fb16d4ed18604106835e59fee72344603afa7 1970-01-01 fbsource commit 5 (with copy of moved path into moved path)
   83da1de25a2a199f98ede29c06bc22e54943cc47 1970-01-01 megarepo commit 1
   99c848e3f5ff3ab7746fb71816748e2ba0d7da36 1970-01-01 fbsource commit 2
   b0474d400edddcabef0a27ead293a6b99ae59490 1970-01-01 ovrsource commit 2
   b06de5da9e40e0da6eda1f7b5c891711106d707b 1970-01-01 fbsource commit 3
   e0cb430152c2dcc47b93a516344e3814ece60d4b 1970-01-01 fbsource commit 10
  default/fbsource/somebook d692e38644b938ccccc4192bd2f507955f3888c5 1970-01-01 non-master commit
  default/master_bookmark * 20*-*-* fbsource commit resume (glob)
  $ export FORGOTTEN_PARENT=$(hg log -T "{node}" -r b0474d400edddcabef0a27ead293a6b99ae59490^)

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
  * using repo "meg-mon" repoid RepositoryId(0) (glob)
  * using repo "fbs-mon" repoid RepositoryId(1) (glob)
  * all is well! (glob)

ovrsource has two problems
(master not matching is not a real problem though)
  $ quiet_grep bookmark -- crossrepo_verify_bookmarks 0 2 | strip_glog | sort
  'ovr-mon' has a bookmark master_bookmark but it points to a commit that has no equivalent in 'meg-mon'. If it's a shared bookmark (e.g. master) that might mean that it points to a commit from another repository
  inconsistent value of forgotten_bookmark: 'ovr-mon' has 36a934b2f08adf9ed2331b0e0dce29522584d085748a9f42d1ca7d1c7787306a, but 'meg-mon' bookmark points to None

update-large-repo-bookmarks won't create commits by itself 
only the syncer can create the commit or they have to be imported some other way
  $ quiet_grep Missing -- crossrepo_verify_bookmarks 0 2 --update-large-repo-bookmarks 
  * Missing outcome for 36a934b2f08adf9ed2331b0e0dce29522584d085748a9f42d1ca7d1c7787306a from small repo (glob)
  [1]

but let's  say we synced that commit manually
  $ megarepo_tool_multirepo --source-repo-id 2 --target-repo-id 0 manual-commit-sync --commit $FORGOTTEN --parents $FORGOTTEN_PARENT --mapping-version-name TEST_VERSION_NAME
  * using repo "ovr-mon" repoid RepositoryId(2) (glob)
  * using repo "meg-mon" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(a30a90f6e3e887c6ee6451dc4c7f9cd352c20495407eb912f9017641e300ca9a)) (glob)
  * changeset resolved as: ChangesetId(Blake2(36a934b2f08adf9ed2331b0e0dce29522584d085748a9f42d1ca7d1c7787306a)) (glob)
  * target cs id is Some(ChangesetId(Blake2(5ec36a79a341b4235da29af79ff591881a994b44c94acaa10c3f583bdef4f2fb))) (glob)

it still doesn't have any data derived
  $ mononoke_newadmin derived-data -R meg-mon exists -T changeset_info -i 5ec36a79a341b4235da29af79ff591881a994b44c94acaa10c3f583bdef4f2fb
  Not Derived: 5ec36a79a341b4235da29af79ff591881a994b44c94acaa10c3f583bdef4f2fb

and tried again, this time in dry run mode with limit 0 to ensure such command wouldn't do anything
  $ crossrepo_verify_bookmarks 0 2 --update-large-repo-bookmarks --no-bookmark-updates --limit 0|& grep bookmark | strip_glog | sort

it still doesn't have any data derived
  $ mononoke_newadmin derived-data -R meg-mon exists -T changeset_info -i 5ec36a79a341b4235da29af79ff591881a994b44c94acaa10c3f583bdef4f2fb
  Not Derived: 5ec36a79a341b4235da29af79ff591881a994b44c94acaa10c3f583bdef4f2fb

and tried again, this time in dry run mode with no limit
  $ quiet_grep bookmark -- crossrepo_verify_bookmarks 0 2 --update-large-repo-bookmarks --no-bookmark-updates --limit 2| strip_glog | sort
  setting ovrsource/forgotten_bookmark 5ec36a79a341b4235da29af79ff591881a994b44c94acaa10c3f583bdef4f2fb
  skipping master_bookmark because it's a common bookmark

and the data is derived
  $ mononoke_newadmin derived-data -R meg-mon exists -T changeset_info -i 5ec36a79a341b4235da29af79ff591881a994b44c94acaa10c3f583bdef4f2fb
  Derived: 5ec36a79a341b4235da29af79ff591881a994b44c94acaa10c3f583bdef4f2fb

and tried again
  $ quiet_grep bookmark -- crossrepo_verify_bookmarks 0 2 --update-large-repo-bookmarks | strip_glog | sort
  setting ovrsource/forgotten_bookmark 5ec36a79a341b4235da29af79ff591881a994b44c94acaa10c3f583bdef4f2fb
  skipping master_bookmark because it's a common bookmark

now the verfication shouldn't return that error
  $ crossrepo_verify_bookmarks 0 2
  * using repo "meg-mon" repoid RepositoryId(0) (glob)
  * using repo "ovr-mon" repoid RepositoryId(2) (glob)
  * 'ovr-mon' has a bookmark master_bookmark but it points to a commit that has no equivalent in 'meg-mon'. If it's a shared bookmark (e.g. master) that might mean that it points to a commit from another repository (glob)
  * found 1 inconsistencies (glob)
  [1]
