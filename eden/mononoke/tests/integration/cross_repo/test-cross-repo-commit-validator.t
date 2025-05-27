# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

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
  > EOF

  $ setup_commitsyncmap
  $ setup_configerator_configs

  $ init_two_small_one_large_repo
  A=e258521a78f8e12bee03bda35489701d887c41fd
  A=8ca76aa82bf928df58db99489fa17938e39774e4
  A=6ebc043d84761f4b77f73e4a2034cf5669bb6a54

-- get some bonsai hashes to avoid magic strings later
  $ FBSOURCE_MASTER_BONSAI=$(mononoke_admin bookmarks --repo-id 1 get master_bookmark)
  $ MEGAREPO_MERGE_BONSAI=$(mononoke_admin bookmarks --repo-id 0 get master_bookmark)

-- insert sync mapping entry
  $ add_synced_commit_mapping_entry 1 $FBSOURCE_MASTER_BONSAI 0 $MEGAREPO_MERGE_BONSAI TEST_VERSION_NAME

-- start mononoke
  $ start_and_wait_for_mononoke_server

-- setup hg client repos
  $ cd "$TESTTMP"
  $ hg clone -q mono:fbs-mon fbs-hg-cnt --noupdate
  $ hg clone -q mono:meg-mon meg-hg-cnt --noupdate

-- create an older version of fbsource_master, with a single simple change
  $ cd "$TESTTMP"/fbs-hg-cnt
  $ hg up -q master_bookmark
  $ createfile fbcode/fbcodefile2_fbsource
  $ createfile arvr/arvrfile2_fbsource
  $ hg -q ci -m "fbsource commit 2"
  $ hg push -r . --to master_bookmark -q
  $ createfile fbcode/fbcodefile3_fbsource
  $ hg -q ci -m "fbsource commit 3"
  $ hg push -r . --to master_bookmark -q

-- sync things to Megarepo
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "INSERT INTO mutable_counters (repo_id, name, value) VALUES (0, 'xreposync_from_1', 1)";
  $ mononoke_x_repo_sync 1 0 tail --catch-up-once |& grep processing
  * processing log entry * (glob)
  * processing log entry * (glob)

  $ flush_mononoke_bookmarks


Record new fbsource master
  $ FBSOURCE_MASTER_BONSAI=$(mononoke_admin bookmarks --repo-id 1 get master_bookmark)
  $ MEGAREPO_MASTER_BONSAI=$(mononoke_admin bookmarks --repo-id 0 get master_bookmark)

Check that we validate the file type

  $ cd "$TESTTMP/meg-hg-cnt"
  $ hg pull -q
  $ hg -q up "master_bookmark~1"

-- create a commit, that is identical to master, but has a different file mode
  $ hg cat fbcode/fbcodefile3_fbsource -r master_bookmark > fbcode/fbcodefile3_fbsource
  $ chmod +x fbcode/fbcodefile3_fbsource
  $ hg ci -qAm "Introduce fbcode/fbcodefile3_fbsource as executable"
  $ hg push --to executable_bookmark --create -q
  $ MEGAREPO_EXECUTABLE_BONSAI=$(mononoke_admin bookmarks --repo-id 0 get executable_bookmark)

-- fake a commit sync mapping between fbsource master and executable commit
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "UPDATE synced_commit_mapping SET large_bcs_id = X'$MEGAREPO_EXECUTABLE_BONSAI' WHERE small_bcs_id = X'$FBSOURCE_MASTER_BONSAI'"

-- run the validator one more time, expect to fail and say it's because of filetypes
  $ REPOIDLARGE=0 validate_commit_sync 4 |& grep "Different filetype"
  * Different filetype for path NonRootMPath("fbcode/fbcodefile3_fbsource"): meg-mon: Executable fbs-mon: Regular (glob)

-- restore the original commit mapping
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "UPDATE synced_commit_mapping SET large_bcs_id = X'$MEGAREPO_MASTER_BONSAI' WHERE small_bcs_id = X'$FBSOURCE_MASTER_BONSAI'"

Check that we validate the file contents
  $ cd "$TESTTMP/meg-hg-cnt"
  $ hg -q up "master_bookmark~1"

-- create a commit, that is identical to master, but has a different file contents
  $ hg cat fbcode/fbcodefile3_fbsource -r master_bookmark > fbcode/fbcodefile3_fbsource
  $ echo "Aeneas was a lively fellow" >> fbcode/fbcodefile3_fbsource
  $ hg ci -qAm "Introduce fbcode/fbcodefile3_fbsource with different content"
  $ hg push --to corrupted_bookmark --create -q
  $ MEGAREPO_CORRUPTED_BONSAI=$(mononoke_admin bookmarks --repo-id 0 get corrupted_bookmark)

-- fake a commit sync mapping between fbsource master and corrupted commit
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "UPDATE synced_commit_mapping SET large_bcs_id = X'$MEGAREPO_CORRUPTED_BONSAI' WHERE small_bcs_id = X'$FBSOURCE_MASTER_BONSAI'"

-- run the validator one more time, expect to fail and say it's because of contents
  $ REPOIDLARGE=0 validate_commit_sync 5 |& grep 'Different contents'
  * Different contents for path NonRootMPath("fbcode/fbcodefile3_fbsource"): meg-mon: * fbs-mon: * (glob)

-- restore the original commit mapping
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "UPDATE synced_commit_mapping SET large_bcs_id = X'$MEGAREPO_MASTER_BONSAI' WHERE small_bcs_id = X'$FBSOURCE_MASTER_BONSAI'"

Check that we pay attention to missing files in small repo, but present in large repo
  $ cd "$TESTTMP/meg-hg-cnt"
  $ hg -q up "master_bookmark~1"

-- create a commit, that is identical to master, but has an extra file (and correspondingly has an extra file, comparing to fbsource master)
  $ echo "Aeneas was a lively fellow" >> fbcode/fbcodefile4_fbsource
  $ hg cat fbcode/fbcodefile3_fbsource -r master_bookmark > fbcode/fbcodefile3_fbsource
  $ hg ci -qAm "Introduce fbcode/fbcodefile3_fbsource with different content"
  $ hg push --to extrafile_bookmark --create -q
  $ MEGAREPO_EXTRAFILE_BONSAI=$(mononoke_admin bookmarks --repo-id 0 get extrafile_bookmark)

-- fake a commit sync mapping between fbsource master and corrupted commit
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "UPDATE synced_commit_mapping SET large_bcs_id = X'$MEGAREPO_EXTRAFILE_BONSAI' WHERE small_bcs_id = X'$FBSOURCE_MASTER_BONSAI'"

-- run the validator one more time, expect to fail and say it's because of contents
  $ REPOIDLARGE=0 validate_commit_sync 6 |& grep "is present in meg-mon"
  * A change to NonRootMPath("fbcode/fbcodefile4_fbsource") is present in meg-mon, but missing in fbs-mon * (glob)

-- restore the original commit mapping
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "UPDATE synced_commit_mapping SET large_bcs_id = X'$MEGAREPO_MASTER_BONSAI' WHERE small_bcs_id = X'$FBSOURCE_MASTER_BONSAI'"

Check that we pay attention to missing files in large repo, but present in small repo
-- Create a large repo commit
  $ cd "$TESTTMP/meg-hg-cnt"
  $ hg -q up "master_bookmark"
  $ echo "Aeneas was a lively fellow" >> fbcode/fbcodefile5
  $ hg ci -qAm "A commit with missing file in large repo"
  $ hg push --to missing_in_large --create -q
  $ MEGAREPO_MISSING_IN_LARGE_BONSAI=$(mononoke_admin bookmarks --repo-id 0 get missing_in_large)

-- Create a small repo commit
  $ cd "$TESTTMP/fbs-hg-cnt"
  $ hg -q up "master_bookmark"
  $ echo "Aeneas was a lively fellow" >> fbcode/fbcodefile5
  $ echo "Aeneas was a lively fellow" >> fbcode/fbcodefile6
  $ hg ci -qAm "A commit with missing file in large repo"
  $ hg push --to missing_in_large --create -q
  $ FBSOURCE_MISSING_IN_LARGE_BONSAI=$(mononoke_admin bookmarks --repo-id 1 get missing_in_large)

-- fake a commit sync mapping between fbsource master and corrupted commit
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "INSERT INTO synced_commit_mapping (small_repo_id, small_bcs_id, large_repo_id, large_bcs_id, sync_map_version_name) VALUES (1, X'$FBSOURCE_MISSING_IN_LARGE_BONSAI', 0, X'$MEGAREPO_MISSING_IN_LARGE_BONSAI', 'TEST_VERSION_NAME')"

-- run the validator one more time, expect to fail and say it's because of contents
  $ REPOIDLARGE=0 validate_commit_sync 7 |& grep "present in fbs-mon, but missing in meg-mon"
  * A change to NonRootMPath("fbcode/fbcodefile6") is present in fbs-mon, but missing in meg-mon * (glob)

Check that for bookmarks_update_log entries, which touch >1 commit in master, we pay
attention to more than just the last commit (successful validation of many commits)
-- Create three commits in the large repo
  $ cd "$TESTTMP/meg-hg-cnt"
  $ hg -q up "master_bookmark"

  $ echo same1 > .fbsource-rest/arvr/tripple_1
  $ hg ci -qAm "Commit 1 of 3"
  $ echo same2 > .fbsource-rest/arvr/tripple_2
  $ hg ci -qAm "Commit 2 of 3"
  $ echo same3 > .fbsource-rest/arvr/tripple_3
  $ hg ci -qAm "Commit 3 of 3"
  $ hg push -q --to master_bookmark
  $ MEGAREPO_MASTER_BONSAI=$(mononoke_admin bookmarks --repo-id 0 get master_bookmark)
  $ MEGAREPO_C1_BONSAI=$(mononoke_admin convert --repo-id 0 --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark~2))
  $ MEGAREPO_C2_BONSAI=$(mononoke_admin convert --repo-id 0 --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark~1))
  $ MEGAREPO_C3_BONSAI=$(mononoke_admin convert --repo-id 0 --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark))

-- Create three commits in the small repo
  $ cd "$TESTTMP/fbs-hg-cnt"
  $ hg -q up "master_bookmark"
  $ echo same1 > arvr/tripple_1
  $ hg ci -qAm "Commit 1 of 3"
  $ echo same2 > arvr/tripple_2
  $ hg ci -qAm "Commit 2 of 3"
  $ echo same3 > arvr/tripple_3
  $ hg ci -qAm "Commit 3 of 3"
  $ hg push -q --to master_bookmark
  $ FBSOURCE_MASTER_BONSAI=$(mononoke_admin bookmarks --repo-id 1 get master_bookmark)
  $ FBSOURCE_C1_BONSAI=$(mononoke_admin convert --repo-id 1 --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark~2))
  $ FBSOURCE_C2_BONSAI=$(mononoke_admin convert --repo-id 1 --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark~1))
  $ FBSOURCE_C3_BONSAI=$(mononoke_admin convert --repo-id 1 --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark))

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
  $ hg -q up "master_bookmark"

  $ echo same1 >> .fbsource-rest/arvr/tripple_1
  $ hg ci -qAm "Commit 1 of 3"
  $ echo different1 >> .fbsource-rest/arvr/tripple_2
  $ hg ci -qAm "Commit 2 of 3"
  $ echo same3 >> .fbsource-rest/arvr/tripple_3
  $ hg ci -qAm "Commit 3 of 3"
  $ hg push -q --to master_bookmark
  $ MEGAREPO_MASTER_BONSAI=$(mononoke_admin bookmarks --repo-id 0 get master_bookmark)
  $ MEGAREPO_C1_BONSAI=$(mononoke_admin convert --repo-id 0 --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark~2))
  $ MEGAREPO_C2_BONSAI=$(mononoke_admin convert --repo-id 0 --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark~1))
  $ MEGAREPO_C3_BONSAI=$(mononoke_admin convert --repo-id 0 --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark))

-- Create three commits in the small repo
  $ cd "$TESTTMP/fbs-hg-cnt"
  $ hg -q up "master_bookmark"
  $ echo same1 >> arvr/tripple_1
  $ hg ci -qAm "Commit 1 of 3"
  $ echo different2 >> arvr/tripple_2
  $ hg ci -qAm "Commit 2 of 3"
  $ echo same3 >> arvr/tripple_3
  $ hg ci -qAm "Commit 3 of 3"
  $ hg push -q --to master_bookmark
  $ FBSOURCE_MASTER_BONSAI=$(mononoke_admin bookmarks --repo-id 1 get master_bookmark)
  $ FBSOURCE_C1_BONSAI=$(mononoke_admin convert --repo-id 1 --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark~2))
  $ FBSOURCE_C2_BONSAI=$(mononoke_admin convert --repo-id 1 --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark~1))
  $ FBSOURCE_C3_BONSAI=$(mononoke_admin convert --repo-id 1 --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark))

-- fake a commit sync mapping between the new commits
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" << ENDOFINSERT
  > INSERT INTO synced_commit_mapping
  >   (small_repo_id, small_bcs_id, large_repo_id, large_bcs_id, sync_map_version_name)
  > VALUES
  >   (1, X'$FBSOURCE_C1_BONSAI', 0, X'$MEGAREPO_C1_BONSAI', 'TEST_VERSION_NAME'),
  >   (1, X'$FBSOURCE_C2_BONSAI', 0, X'$MEGAREPO_C2_BONSAI', 'TEST_VERSION_NAME'),
  >   (1, X'$FBSOURCE_C3_BONSAI', 0, X'$MEGAREPO_C3_BONSAI', 'TEST_VERSION_NAME');
  > ENDOFINSERT

-- run the validator, check that commits are equivalent
  $ REPOIDLARGE=0 validate_commit_sync 9 |& grep -E "(Preparing entry|Different contents)"
  * Preparing entry Entry 9 (1/3); book: master_bookmark; cs_id: ChangesetId(Blake2(*)); remaining queue: 0 (glob)
  * Preparing entry Entry 9 (2/3); book: master_bookmark; cs_id: ChangesetId(Blake2(*)); remaining queue: 0 (glob)
  * Different contents for path NonRootMPath("arvr/tripple_2"): meg-mon: ContentId(Blake2(*)) fbs-mon: ContentId(Blake2(*)) (glob)

Check that we validate the topological order
-- Create three commits in the large repo
  $ cd "$TESTTMP/meg-hg-cnt"
  $ hg -q up "master_bookmark"

  $ hg ci -qAm "Commit 1 of 2" --config ui.allowemptycommit=True
  $ hg ci -qAm "Commit 2 of 2" --config ui.allowemptycommit=True
  $ hg push -q --to master_bookmark
  $ MEGAREPO_MASTER_BONSAI=$(mononoke_admin bookmarks --repo-id 0 get master_bookmark)
  $ MEGAREPO_C1_BONSAI=$(mononoke_admin convert --repo-id 0 --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark~1))
  $ MEGAREPO_C2_BONSAI=$(mononoke_admin convert --repo-id 0 --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark))

-- Create three commits in the small repo
  $ cd "$TESTTMP/fbs-hg-cnt"
  $ hg -q up "master_bookmark"
  $ hg ci -qAm "Commit 1 of 2" --config ui.allowemptycommit=True
  $ hg ci -qAm "Commit 2 of 2" --config ui.allowemptycommit=True
  $ hg push -q --to master_bookmark
  $ FBSOURCE_MASTER_BONSAI=$(mononoke_admin bookmarks --repo-id 1 get master_bookmark)
  $ FBSOURCE_C1_BONSAI=$(mononoke_admin convert --repo-id 1 --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark~1))
  $ FBSOURCE_C2_BONSAI=$(mononoke_admin convert --repo-id 1 --from hg --to bonsai $(hg log -T"{node}" -r master_bookmark))

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
  $ hg up -q null
  $ mkdir -p .fbsource-rest/arvr && echo root > .fbsource-rest/arvr/root
  $ hg ci -qAm "Root commit"
  $ hg push -r . --to another_root --force --create -q
  $ MEGAREPO_NEWROOT_BONSAI=$(mononoke_admin bookmarks --repo-id 0 get another_root)

  $ cd "$TESTTMP/fbs-hg-cnt"
  $ hg up -q null
  $ mkdir arvr && echo root > arvr/root
  $ hg ci -qAm "Root commit"
  $ hg push -r . --to another_root --force --create -q
  $ FBSOURCE_NEWROOT_BONSAI=$(mononoke_admin bookmarks --repo-id 1 get another_root)

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
