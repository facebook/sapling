# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

setup configuration
Disable bookmarks cache because bookmarks are modified by two separate processes
  $ REPOTYPE="blob_files"
  $ REPOID=0 REPONAME=meg-mon setup_common_config $REPOTYPE
  $ REPOID=1 REPONAME=fbs-mon setup_common_config $REPOTYPE
  $ REPOID=2 REPONAME=ovr-mon setup_common_config $REPOTYPE  # ovr-mon just exists here to make the test sync config work

  $ cat >> "$HGRCPATH" <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > [extensions]
  > amend=
  > pushrebase=
  > remotenames=
  > EOF

  $ setup_commitsyncmap
  $ setup_configerator_configs

-- setup hg server repos

  $ function createfile { mkdir -p "$(dirname  $1)" && echo "$1" > "$1" && hg add -q "$1"; }
  $ function createfile_with_content { mkdir -p "$(dirname  $1)" && echo "$2" > "$1" && hg add -q "$1"; }

-- init fbsource
  $ cd $TESTTMP
  $ hginit_treemanifest fbs-hg-srv
  $ cd fbs-hg-srv
-- create an initial commit, which will be the last_synced_commit
  $ createfile fbcode/fbcodefile_fbsource
  $ createfile arvr/arvrfile_fbsource
  $ createfile otherfile_fbsource
  $ hg -q ci -m "fbsource commit 1" && hg book -ir . master_bookmark

-- init ovrsource
  $ cd $TESTTMP
  $ hginit_treemanifest ovr-hg-srv
  $ cd ovr-hg-srv
  $ createfile fbcode/fbcodefile_ovrsource
  $ createfile arvr/arvrfile_ovrsource
  $ createfile otherfile_ovrsource
  $ createfile Research/researchfile_ovrsource
  $ hg -q ci -m "ovrsource commit 1" && hg book -r . master_bookmark

-- init megarepo - note that some paths are shifted, but content stays the same
  $ cd $TESTTMP
  $ hginit_treemanifest meg-hg-srv
  $ cd meg-hg-srv
  $ createfile fbcode/fbcodefile_fbsource
  $ createfile_with_content .fbsource-rest/arvr/arvrfile_fbsource arvr/arvrfile_fbsource
  $ createfile otherfile_fbsource
  $ createfile_with_content .ovrsource-rest/fbcode/fbcodefile_ovrsource fbcode/fbcodefile_ovrsource
  $ createfile arvr/arvrfile_ovrsource
  $ createfile_with_content arvr-legacy/otherfile_ovrsource otherfile_ovrsource
  $ createfile_with_content arvr-legacy/Research/researchfile_ovrsource Research/researchfile_ovrsource
  $ hg -q ci -m "megarepo commit 1"
  $ hg book -r . master_bookmark

-- blobimport hg servers repos into Mononoke repos
  $ cd "$TESTTMP"
  $ REPOID=0 blobimport meg-hg-srv/.hg meg-mon
  $ REPOID=1 blobimport fbs-hg-srv/.hg fbs-mon

-- get some bonsai hashes to avoid magic strings later
  $ FBSOURCE_MASTER_BONSAI=$(get_bonsai_bookmark 1 master_bookmark)
  $ MEGAREPO_MERGE_BONSAI=$(get_bonsai_bookmark 0 master_bookmark)

-- insert sync mapping entry
  $ add_synced_commit_mapping_entry 1 $FBSOURCE_MASTER_BONSAI 0 $MEGAREPO_MERGE_BONSAI TEST_VERSION_NAME

-- setup hg client repos
  $ cd "$TESTTMP"
  $ hgclone_treemanifest ssh://user@dummy/fbs-hg-srv fbs-hg-cnt --noupdate
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
  $ createfile fbcode/fbcodefile3_fbsource
  $ hg -q ci -m "fbsource commit 3"
  $ REPONAME=fbs-mon hgmn push -r . --to master_bookmark -q

-- sync things to Megarepo
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "INSERT INTO mutable_counters (repo_id, name, value) VALUES (0, 'xreposync_from_1', 1)";
  $ mononoke_x_repo_sync 1 0 tail --catch-up-once |& grep processing
  * processing log entry * (glob)
  * processing log entry * (glob)

  $ flush_mononoke_bookmarks


Record new fbsource master
  $ FBSOURCE_MASTER_BONSAI=$(get_bonsai_bookmark 1 master_bookmark)
  $ MEGAREPO_MASTER_BONSAI=$(get_bonsai_bookmark 0 master_bookmark)

Check that we validate the file type

  $ cd "$TESTTMP/meg-hg-cnt"
  $ REPONAME=meg-mon hgmn pull -q
  $ REPONAME=meg-mon hgmn -q up "master_bookmark~1"

-- create a commit, that is identical to master, but has a different file mode
  $ REPONAME=meg-mon hgmn cat fbcode/fbcodefile3_fbsource -r master_bookmark > fbcode/fbcodefile3_fbsource
  $ chmod +x fbcode/fbcodefile3_fbsource
  $ hg ci -qAm "Introduce fbcode/fbcodefile3_fbsource as executable"
  $ REPONAME=meg-mon hgmn push --to executable_bookmark --create -q
  $ MEGAREPO_EXECUTABLE_BONSAI=$(get_bonsai_bookmark 0 executable_bookmark)

-- fake a commit sync mapping between fbsource master and executable commit
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "UPDATE synced_commit_mapping SET large_bcs_id = X'$MEGAREPO_EXECUTABLE_BONSAI' WHERE small_bcs_id = X'$FBSOURCE_MASTER_BONSAI'"

-- run the validator one more time, expect to fail and say it's because of filetypes
  $ REPOIDLARGE=0 validate_commit_sync 4 |& grep "Different filetype"
  * Different filetype for path MPath("fbcode/fbcodefile3_fbsource"): meg-mon: Executable fbs-mon: Regular (glob)

-- restore the original commit mapping
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "UPDATE synced_commit_mapping SET large_bcs_id = X'$MEGAREPO_MASTER_BONSAI' WHERE small_bcs_id = X'$FBSOURCE_MASTER_BONSAI'"

Check that we validate the file contents
  $ cd "$TESTTMP/meg-hg-cnt"
  $ REPONAME=meg-mon hgmn -q up "master_bookmark~1"

-- create a commit, that is identical to master, but has a different file contents
  $ REPONAME=meg-mon hgmn cat fbcode/fbcodefile3_fbsource -r master_bookmark > fbcode/fbcodefile3_fbsource
  $ echo "Aeneas was a lively fellow" >> fbcode/fbcodefile3_fbsource
  $ hg ci -qAm "Introduce fbcode/fbcodefile3_fbsource with different content"
  $ REPONAME=meg-mon hgmn push --to corrupted_bookmark --create -q
  $ MEGAREPO_CORRUPTED_BONSAI=$(get_bonsai_bookmark 0 corrupted_bookmark)

-- fake a commit sync mapping between fbsource master and corrupted commit
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "UPDATE synced_commit_mapping SET large_bcs_id = X'$MEGAREPO_CORRUPTED_BONSAI' WHERE small_bcs_id = X'$FBSOURCE_MASTER_BONSAI'"

-- run the validator one more time, expect to fail and say it's because of contents
  $ REPOIDLARGE=0 validate_commit_sync 5 |& grep 'Different contents'
  * Different contents for path MPath("fbcode/fbcodefile3_fbsource"): meg-mon: * fbs-mon: * (glob)

-- restore the original commit mapping
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "UPDATE synced_commit_mapping SET large_bcs_id = X'$MEGAREPO_MASTER_BONSAI' WHERE small_bcs_id = X'$FBSOURCE_MASTER_BONSAI'"

Check that we pay attention to missing files in small repo, but present in large repo
  $ cd "$TESTTMP/meg-hg-cnt"
  $ REPONAME=meg-mon hgmn -q up "master_bookmark~1"

-- create a commit, that is identical to master, but has an extra file (and correspondingly has an extra file, comparing to fbsource master)
  $ echo "Aeneas was a lively fellow" >> fbcode/fbcodefile4_fbsource
  $ REPONAME=meg-mon hgmn cat fbcode/fbcodefile3_fbsource -r master_bookmark > fbcode/fbcodefile3_fbsource
  $ hg ci -qAm "Introduce fbcode/fbcodefile3_fbsource with different content"
  $ REPONAME=meg-mon hgmn push --to extrafile_bookmark --create -q
  $ MEGAREPO_EXTRAFILE_BONSAI=$(get_bonsai_bookmark 0 extrafile_bookmark)

-- fake a commit sync mapping between fbsource master and corrupted commit
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "UPDATE synced_commit_mapping SET large_bcs_id = X'$MEGAREPO_EXTRAFILE_BONSAI' WHERE small_bcs_id = X'$FBSOURCE_MASTER_BONSAI'"

-- run the validator one more time, expect to fail and say it's because of contents
  $ REPOIDLARGE=0 validate_commit_sync 6 |& grep "is present in meg-mon"
  * A change to MPath("fbcode/fbcodefile4_fbsource") is present in meg-mon, but missing in fbs-mon * (glob)

-- restore the original commit mapping
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "UPDATE synced_commit_mapping SET large_bcs_id = X'$MEGAREPO_MASTER_BONSAI' WHERE small_bcs_id = X'$FBSOURCE_MASTER_BONSAI'"

Check that we pay attention to missing files in large repo, but present in small repo
-- Create a large repo commit
  $ cd "$TESTTMP/meg-hg-cnt"
  $ REPONAME=meg-mon hgmn -q up "master_bookmark"
  $ echo "Aeneas was a lively fellow" >> fbcode/fbcodefile5
  $ hg ci -qAm "A commit with missing file in large repo"
  $ REPONAME=meg-mon hgmn push --to missing_in_large --create -q
  $ MEGAREPO_MISSING_IN_LARGE_BONSAI=$(get_bonsai_bookmark 0 missing_in_large)

-- Create a small repo commit
  $ cd "$TESTTMP/fbs-hg-cnt"
  $ REPONAME=fbs-mon hgmn -q up "master_bookmark"
  $ echo "Aeneas was a lively fellow" >> fbcode/fbcodefile5
  $ echo "Aeneas was a lively fellow" >> fbcode/fbcodefile6
  $ hg ci -qAm "A commit with missing file in large repo"
  $ REPONAME=fbs-mon hgmn push --to missing_in_large --create -q
  $ FBSOURCE_MISSING_IN_LARGE_BONSAI=$(get_bonsai_bookmark 1 missing_in_large)

-- fake a commit sync mapping between fbsource master and corrupted commit
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "INSERT INTO synced_commit_mapping (small_repo_id, small_bcs_id, large_repo_id, large_bcs_id, sync_map_version_name) VALUES (1, X'$FBSOURCE_MISSING_IN_LARGE_BONSAI', 0, X'$MEGAREPO_MISSING_IN_LARGE_BONSAI', 'TEST_VERSION_NAME')"

-- run the validator one more time, expect to fail and say it's because of contents
  $ REPOIDLARGE=0 validate_commit_sync 7 |& grep "present in fbs-mon, but missing in meg-mon"
  * A change to MPath("fbcode/fbcodefile6") is present in fbs-mon, but missing in meg-mon * (glob)

Check that for bookmarks_update_log entries, which touch >1 commit in master, we pay
attention to more than just the last commit (successful validation of many commits)
-- Create three commits in the large repo
  $ cd "$TESTTMP/meg-hg-cnt"
  $ REPONAME=meg-mon hgmn -q up "master_bookmark"

  $ echo same1 > .fbsource-rest/arvr/tripple_1
  $ hg ci -qAm "Commit 1 of 3"
  $ echo same2 > .fbsource-rest/arvr/tripple_2
  $ hg ci -qAm "Commit 2 of 3"
  $ echo same3 > .fbsource-rest/arvr/tripple_3
  $ hg ci -qAm "Commit 3 of 3"
  $ REPONAME=meg-mon hgmn push -q --to master_bookmark
  $ MEGAREPO_MASTER_BONSAI=$(get_bonsai_bookmark 0 master_bookmark)
  $ MEGAREPO_C1_BONSAI=$(REPOID=0 mononoke_admin convert --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark~2) 2>/dev/null)
  $ MEGAREPO_C2_BONSAI=$(REPOID=0 mononoke_admin convert --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark~1) 2>/dev/null)
  $ MEGAREPO_C3_BONSAI=$(REPOID=0 mononoke_admin convert --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark) 2>/dev/null)

-- Create three commits in the small repo
  $ cd "$TESTTMP/fbs-hg-cnt"
  $ REPONAME=fbs-mon hgmn -q up "master_bookmark"
  $ echo same1 > arvr/tripple_1
  $ hg ci -qAm "Commit 1 of 3"
  $ echo same2 > arvr/tripple_2
  $ hg ci -qAm "Commit 2 of 3"
  $ echo same3 > arvr/tripple_3
  $ hg ci -qAm "Commit 3 of 3"
  $ REPONAME=fbs-mon hgmn push -q --to master_bookmark
  $ FBSOURCE_MASTER_BONSAI=$(get_bonsai_bookmark 1 master_bookmark)
  $ FBSOURCE_C1_BONSAI=$(REPOID=1 mononoke_admin convert --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark~2) 2>/dev/null)
  $ FBSOURCE_C2_BONSAI=$(REPOID=1 mononoke_admin convert --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark~1) 2>/dev/null)
  $ FBSOURCE_C3_BONSAI=$(REPOID=1 mononoke_admin convert --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark) 2>/dev/null)

-- fake a commit sync mapping between the new commits
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" << ENDOFINSERT
  > INSERT INTO synced_commit_mapping
  >   (small_repo_id, small_bcs_id, large_repo_id, large_bcs_id, sync_map_version_name)
  > VALUES 
  >   (1, X'$FBSOURCE_C1_BONSAI', 0, X'$MEGAREPO_C1_BONSAI', 'TEST_VERSION_NAME'),
  >   (1, X'$FBSOURCE_C2_BONSAI', 0, X'$MEGAREPO_C2_BONSAI', 'TEST_VERSION_NAME'),
  >   (1, X'$FBSOURCE_C3_BONSAI', 0, X'$MEGAREPO_C3_BONSAI', 'TEST_VERSION_NAME');
  > ENDOFINSERT

-- run the validator, check that commits are eqiuvalent
  $ REPOIDLARGE=0 validate_commit_sync 8 |& grep "Validated entry"
  * Validated entry: Entry 8 (1/3) (glob)
  * Validated entry: Entry 8 (2/3) (glob)
  * Validated entry: Entry 8 (3/3) (glob)

Check that for bookmarks_update_log entries, which touch >1 commit in master, we pay
attention to more than just the last commit (failed validation of inner commit)
-- Create three commits in the large repo
  $ cd "$TESTTMP/meg-hg-cnt"
  $ REPONAME=meg-mon hgmn -q up "master_bookmark"

  $ echo same1 >> .fbsource-rest/arvr/tripple_1
  $ hg ci -qAm "Commit 1 of 3"
  $ echo different1 >> .fbsource-rest/arvr/tripple_2
  $ hg ci -qAm "Commit 2 of 3"
  $ echo same3 >> .fbsource-rest/arvr/tripple_3
  $ hg ci -qAm "Commit 3 of 3"
  $ REPONAME=meg-mon hgmn push -q --to master_bookmark
  $ MEGAREPO_MASTER_BONSAI=$(get_bonsai_bookmark 0 master_bookmark)
  $ MEGAREPO_C1_BONSAI=$(REPOID=0 mononoke_admin convert --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark~2) 2>/dev/null)
  $ MEGAREPO_C2_BONSAI=$(REPOID=0 mononoke_admin convert --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark~1) 2>/dev/null)
  $ MEGAREPO_C3_BONSAI=$(REPOID=0 mononoke_admin convert --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark) 2>/dev/null)

-- Create three commits in the small repo
  $ cd "$TESTTMP/fbs-hg-cnt"
  $ REPONAME=fbs-mon hgmn -q up "master_bookmark"
  $ echo same1 >> arvr/tripple_1
  $ hg ci -qAm "Commit 1 of 3"
  $ echo different2 >> arvr/tripple_2
  $ hg ci -qAm "Commit 2 of 3"
  $ echo same3 >> arvr/tripple_3
  $ hg ci -qAm "Commit 3 of 3"
  $ REPONAME=fbs-mon hgmn push -q --to master_bookmark
  $ FBSOURCE_MASTER_BONSAI=$(get_bonsai_bookmark 1 master_bookmark)
  $ FBSOURCE_C1_BONSAI=$(REPOID=1 mononoke_admin convert --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark~2) 2>/dev/null)
  $ FBSOURCE_C2_BONSAI=$(REPOID=1 mononoke_admin convert --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark~1) 2>/dev/null)
  $ FBSOURCE_C3_BONSAI=$(REPOID=1 mononoke_admin convert --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark) 2>/dev/null)

-- fake a commit sync mapping between the new commits
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" << ENDOFINSERT
  > INSERT INTO synced_commit_mapping
  >   (small_repo_id, small_bcs_id, large_repo_id, large_bcs_id, sync_map_version_name)
  > VALUES 
  >   (1, X'$FBSOURCE_C1_BONSAI', 0, X'$MEGAREPO_C1_BONSAI', 'TEST_VERSION_NAME'),
  >   (1, X'$FBSOURCE_C2_BONSAI', 0, X'$MEGAREPO_C2_BONSAI', 'TEST_VERSION_NAME'),
  >   (1, X'$FBSOURCE_C3_BONSAI', 0, X'$MEGAREPO_C3_BONSAI', 'TEST_VERSION_NAME');
  > ENDOFINSERT

-- run the validator, check that commits are eqiuvalent
  $ REPOIDLARGE=0 validate_commit_sync 9 |& grep -E "(Preparing entry|Different contents)"
  * Preparing entry Entry 9 (1/3); book: master_bookmark; cs_id: ChangesetId(Blake2(*)); remaining queue: 0 (glob)
  * Preparing entry Entry 9 (2/3); book: master_bookmark; cs_id: ChangesetId(Blake2(*)); remaining queue: 0 (glob)
  * Different contents for path MPath("arvr/tripple_2"): meg-mon: ContentId(Blake2(*)) fbs-mon: ContentId(Blake2(*)) (glob)

Check that we validate the topological order
-- Create three commits in the large repo
  $ cd "$TESTTMP/meg-hg-cnt"
  $ REPONAME=meg-mon hgmn -q up "master_bookmark"

  $ hg ci -qAm "Commit 1 of 2" --config ui.allowemptycommit=True
  $ hg ci -qAm "Commit 2 of 2" --config ui.allowemptycommit=True
  $ REPONAME=meg-mon hgmn push -q --to master_bookmark
  $ MEGAREPO_MASTER_BONSAI=$(get_bonsai_bookmark 0 master_bookmark)
  $ MEGAREPO_C1_BONSAI=$(REPOID=0 mononoke_admin convert --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark~1) 2>/dev/null)
  $ MEGAREPO_C2_BONSAI=$(REPOID=0 mononoke_admin convert --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark) 2>/dev/null)

-- Create three commits in the small repo
  $ cd "$TESTTMP/fbs-hg-cnt"
  $ REPONAME=fbs-mon hgmn -q up "master_bookmark"
  $ hg ci -qAm "Commit 1 of 2" --config ui.allowemptycommit=True
  $ hg ci -qAm "Commit 2 of 2" --config ui.allowemptycommit=True
  $ REPONAME=fbs-mon hgmn push -q --to master_bookmark
  $ FBSOURCE_MASTER_BONSAI=$(get_bonsai_bookmark 1 master_bookmark)
  $ FBSOURCE_C1_BONSAI=$(REPOID=1 mononoke_admin convert --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark~1) 2>/dev/null)
  $ FBSOURCE_C2_BONSAI=$(REPOID=1 mononoke_admin convert --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark) 2>/dev/null)

-- fake a commit sync mapping between the new commits
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" << ENDOFINSERT
  > INSERT INTO synced_commit_mapping
  >   (small_repo_id, small_bcs_id, large_repo_id, large_bcs_id, sync_map_version_name)
  > VALUES 
  >   (1, X'$FBSOURCE_C1_BONSAI', 0, X'$MEGAREPO_C2_BONSAI', 'TEST_VERSION_NAME'),
  >   (1, X'$FBSOURCE_C2_BONSAI', 0, X'$MEGAREPO_C1_BONSAI', 'TEST_VERSION_NAME');
  > ENDOFINSERT

-- run the validator, check that commits are eqiuvalent
  $ REPOIDLARGE=0 validate_commit_sync 10 |& grep -E "(topological order|is not an ancestor)"
  * validating topological order for *<->* (glob)
  * Error while verifying against TEST_VERSION_NAME: * (remapping of parent * of * in 1) is not an ancestor of * in 0 (glob)
  * Execution error: * (remapping of parent * of * in 1) is not an ancestor of * in 0 (glob)

Check that we validate the newly-added root commits
  $ cd "$TESTTMP/meg-hg-cnt"
  $ REPONAME=meg-mon hgmn up -q null
  $ mkdir -p .fbsource-rest/arvr && echo root > .fbsource-rest/arvr/root
  $ hg ci -qAm "Root commit"
  $ REPONAME=meg-mon hgmn push -r . --to another_root --force --create -q
  $ MEGAREPO_NEWROOT_BONSAI=$(get_bonsai_bookmark 0 another_root)

  $ cd "$TESTTMP/fbs-hg-cnt"
  $ REPONAME=fbs-mon hgmn up -q null
  $ mkdir arvr && echo root > arvr/root
  $ hg ci -qAm "Root commit"
  $ REPONAME=fbs-mon hgmn push -r . --to another_root --force --create -q
  $ FBSOURCE_NEWROOT_BONSAI=$(get_bonsai_bookmark 1 another_root)

-- fake a commit sync mapping between the new commits
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" << ENDOFINSERT
  > INSERT INTO synced_commit_mapping
  >   (small_repo_id, small_bcs_id, large_repo_id, large_bcs_id, sync_map_version_name)
  > VALUES 
  >   (1, X'$FBSOURCE_NEWROOT_BONSAI', 0, X'$MEGAREPO_NEWROOT_BONSAI', 'TEST_VERSION_NAME');
  > ENDOFINSERT

-- run the validator, check that commits are (1) validated (2) different
  $ REPOIDLARGE=0 validate_commit_sync 11 |& grep -E '(is a root|Validated entry)'
  * is a root cs. Grabbing its entire manifest (glob)
  * is a root cs. Grabbing its entire manifest (glob)
  * Validated entry: * (glob)
