# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

  $ init_two_small_one_large_repo

-- get some bonsai hashes to avoid magic strings later
  $ FBSOURCE_MASTER_BONSAI=$(mononoke_admin bookmarks --repo-id 1 get master_bookmark)
  $ OVRSOURCE_MASTER_BONSAI=$(mononoke_admin bookmarks --repo-id 2 get master_bookmark)
  $ MEGAREPO_MERGE_BONSAI=$(mononoke_admin bookmarks --repo-id 0 get master_bookmark)

-- insert sync mapping entry
  $ add_synced_commit_mapping_entry 1 $FBSOURCE_MASTER_BONSAI 0 $MEGAREPO_MERGE_BONSAI TEST_VERSION_NAME
  $ add_synced_commit_mapping_entry 2 $OVRSOURCE_MASTER_BONSAI 0 $MEGAREPO_MERGE_BONSAI TEST_VERSION_NAME

-- setup hg client repos
  $ cd "$TESTTMP"
  $ setconfig remotenames.selectivepulldefault=master_bookmark,somebookmark
  $ hg clone -q mono:fbs-mon fbs-hg-cnt --noupdate
  $ hg clone -q mono:ovr-mon ovr-hg-cnt --noupdate
  $ hg clone -q mono:meg-mon meg-hg-cnt --noupdate

-- start mononoke
  $ start_and_wait_for_mononoke_server
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "INSERT INTO mutable_counters (repo_id, name, value) VALUES (0, 'xreposync_from_2', 0)";
  $ mononoke_x_repo_sync 2 0 tail --catch-up-once |& grep -E '(processing|skipping)'
  * processing log entry * (glob)

-- push to a bookmark that won't be synced
  $ cd "$TESTTMP"/ovr-hg-cnt
  $ hg up -q master_bookmark
  $ createfile arvr/branchfile
  $ hg -q ci -m "ovrsource branch commit"
  $ hg push -r . --to anotherbookmark -q --create
  $ hg up -q .^
  $ createfile arvr/somefile
  $ hg -q ci -m "ovrsource commit 2"
  $ hg push -r . --to somebookmark -q --create

-- now push to master
  $ hg up -q master_bookmark
  $ createfile arvr/newfile
  $ hg -q ci -m "ovrsource commit 3"
  $ hg push -r . --to master_bookmark -q
  $ hg up somebookmark -q
  $ createfile arvr/somefile2
  $ hg -q ci -m "ovrsource commit 4"
  $ hg push -r . --to somebookmark -q --create

  $ with_stripped_logs mononoke_x_repo_sync 2 0 tail --bookmark-regex "master_bookmark" \
  > --catch-up-once |& grep -E '(processing|skipping)'
  skipping log entry #2 for anotherbookmark
  skipping log entry #3 for somebookmark
  processing log entry #4
  skipping log entry #5 for somebookmark
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select * from mutable_counters where name = 'xreposync_from_2'";
  0|xreposync_from_2|5


-- use verify-bookmarks command. One inconsistency.
  $ with_stripped_logs crossrepo_verify_bookmarks 2 0
  inconsistent value of *, but 'meg-mon' bookmark points to None (glob)
  inconsistent value of *, but 'meg-mon' bookmark points to None (glob)
  Error: found 2 inconsistencies
 

-- use verify-bookmarks, but passing a regex.
  $ with_stripped_logs crossrepo_verify_bookmarks 2 0 --update-large-repo-bookmarks \
  > --no-bookmark-updates --bookmark-regex "master_bookmark"
  all is well!


-- updating large repo bookmark will not work, bc there are unsynced commits.
  $ with_stripped_logs crossrepo_verify_bookmarks 2 0 --update-large-repo-bookmarks \
  > --no-bookmark-updates
  found 2 inconsistencies, trying to update them...
  Error: Missing outcome for * from small repo (glob)

-- sync the missing commits
  $ with_stripped_logs mononoke_x_repo_sync 2 0 once --bookmark-regex ".+bookmark"
  Starting session with id * (glob)
  Starting up X Repo Sync from small repo ovr-mon to large repo meg-mon
  Syncing 3 commits and all of their unsynced ancestors
  Checking if 1c0ab9ee548f45eaabe8e81d8a67b2cd0748dff1453fccbed0a67a153c1bb48b is already synced 2->0
  1c0ab9ee548f45eaabe8e81d8a67b2cd0748dff1453fccbed0a67a153c1bb48b is already synced
  Checking if 195e3fd3952a97ff2714800a399751f1f52ac87454e745f9871403db5a377696 is already synced 2->0
  1 unsynced ancestors of 195e3fd3952a97ff2714800a399751f1f52ac87454e745f9871403db5a377696
  syncing 195e3fd3952a97ff2714800a399751f1f52ac87454e745f9871403db5a377696
  changeset 195e3fd3952a97ff2714800a399751f1f52ac87454e745f9871403db5a377696 synced as 5c59f83b8a6fb9b56902be03e0bda3d7bbf2bd629a1caead56a4a8385e5cc8f5 in * (glob)
  successful sync
  Checking if de4dfe2c590fda9c42549a2f6a2ea8eb7fab5b3b9690573e499e5814fff5ba7c is already synced 2->0
  2 unsynced ancestors of de4dfe2c590fda9c42549a2f6a2ea8eb7fab5b3b9690573e499e5814fff5ba7c
  syncing cdd71132c0062819766a1d6eb6c0b3384a4db3070c9936bd98920c142506f940
  changeset cdd71132c0062819766a1d6eb6c0b3384a4db3070c9936bd98920c142506f940 synced as 9d6f30edf25f7b5d72295ff3dae947aee3f74dbbf09077f16e5ce0c970875b3e in * (glob)
  syncing de4dfe2c590fda9c42549a2f6a2ea8eb7fab5b3b9690573e499e5814fff5ba7c
  changeset de4dfe2c590fda9c42549a2f6a2ea8eb7fab5b3b9690573e499e5814fff5ba7c synced as 160490009025590980be0fd659300a0c9fc6cf39b05f2a922cf844f11919fc5d in * (glob)
  successful sync
  X Repo Sync execution finished from small repo ovr-mon to large repo meg-mon

-- now update the inconsistent bookmarks
  $ with_stripped_logs crossrepo_verify_bookmarks 2 0 --update-large-repo-bookmarks
  found 2 inconsistencies, trying to update them...
  setting ovrsource/* (glob)
  setting ovrsource/* (glob)

-- mutable counter was not updated, because the once command was used
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select * from mutable_counters where name = 'xreposync_from_2'";
  0|xreposync_from_2|5
