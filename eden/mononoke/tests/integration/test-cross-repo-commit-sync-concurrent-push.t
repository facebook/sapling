# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

  $ large_small_megarepo_config
  $ init_large_small_repo
  Adding synced mapping entry
  Starting Mononoke server

-- Show the small and the large repo from the common config (init_large_small_repo, see library-push-redirector.sh)
  $ mononoke_newadmin changelog -R small-mon graph -i $S_B -M
  o  message: first post-move commit
  │
  o  message: pre-move commit

  $ mononoke_newadmin changelog -R large-mon graph -i $L_C -M
  o  message: first post-move commit
  │
  o  message: move commit
  │
  o  message: pre-move commit

-- Simultaneously push a non-pushrebase bookmark (other_bookmark) one commit forward to S_C and a pushrebase bookmark master_bookmark) two commits forward to S_D
  $ testtool_drawdag -R small-mon << EOF
  > S_A-S_B-S_C-S_D
  > # exists: S_A $S_A
  > # exists: S_B $S_B
  > # bookmark: S_D master_bookmark
  > # bookmark: S_C other_bookmark
  > EOF
  S_A=c74140f562eda7c378d4e8d68e4828239617dd51806f3ccb220433a3ea1a6353
  S_B=1ba347e63a4bf200944c22ade8dbea038dd271ef97af346ba4ccfaaefb10dd4d
  S_C=6899eb0af1d64df45683e6bf22c8b82593b22539dec09394f516f944f6fa8c12
  S_D=542a68bb4fd5a7ba5a047a0bb29a48d660c0ea5114688d00b11658313e8f1e6b

-- The small repo now looks like this
  $ mononoke_newadmin changelog -R small-mon graph -i $S_D -M
  o  message: S_D
  │
  o  message: S_C
  │
  o  message: first post-move commit
  │
  o  message: pre-move commit

-- Sync after both bookmark moves happened
-- The first bookmark move of other_bookmark to S_C is replicated correctly
-- The second bookmark move of master_bookmark to S_D fails with "No common pushrebase root for master_bookmark". This is a bug.
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "INSERT INTO mutable_counters (repo_id, name, value) VALUES (0, 'xreposync_from_1', 0)";
  $ mononoke_x_repo_sync 1 0 tail --catch-up-once
  * Starting session with id * (glob)
  * queue size is 3 (glob)
  * processing log entry #1 (glob)
  * 0 unsynced ancestors of 1ba347e63a4bf200944c22ade8dbea038dd271ef97af346ba4ccfaaefb10dd4d (glob)
  * successful sync bookmark update log #1 (glob)
  * processing log entry #2 (glob)
  * 1 unsynced ancestors of 6899eb0af1d64df45683e6bf22c8b82593b22539dec09394f516f944f6fa8c12 (glob)
  * syncing 6899eb0af1d64df45683e6bf22c8b82593b22539dec09394f516f944f6fa8c12 (glob)
  * changeset 6899eb0af1d64df45683e6bf22c8b82593b22539dec09394f516f944f6fa8c12 synced as d06c956180c43660142dabd61da09e9c6d2b19a53f43fee62b5f919789e24411 * (glob)
  * successful sync bookmark update log #2 (glob)
  * processing log entry #3 (glob)
  * 1 unsynced ancestors of 542a68bb4fd5a7ba5a047a0bb29a48d660c0ea5114688d00b11658313e8f1e6b (glob)
  * syncing 542a68bb4fd5a7ba5a047a0bb29a48d660c0ea5114688d00b11658313e8f1e6b via pushrebase for master_bookmark (glob)
  * Syncing 542a68bb4fd5a7ba5a047a0bb29a48d660c0ea5114688d00b11658313e8f1e6b failed in *: Pushrebase of synced commit failed - check config for overlaps: Error(No common pushrebase root for master_bookmark, all possible roots: {ChangesetId(Blake2(d06c956180c43660142dabd61da09e9c6d2b19a53f43fee62b5f919789e24411))}) (glob)
  * failed to sync bookmark update log #3, Pushrebase of synced commit failed - check config for overlaps: Error(No common pushrebase root for master_bookmark, all possible roots: {ChangesetId(Blake2(d06c956180c43660142dabd61da09e9c6d2b19a53f43fee62b5f919789e24411))}) (glob)
  * Execution error: Pushrebase of synced commit failed - check config for overlaps: Error(No common pushrebase root for master_bookmark, all possible roots: {ChangesetId(Blake2(d06c956180c43660142dabd61da09e9c6d2b19a53f43fee62b5f919789e24411))}) (glob)
  * Execution failed (glob)
  [1]

-- Show the bookmarks after the sync
  $ mononoke_newadmin bookmarks --repo-name large-mon list
  d06c956180c43660142dabd61da09e9c6d2b19a53f43fee62b5f919789e24411 bookprefix/other_bookmark
  3e020372209167db53084d8295a9d94bb1cd654e19711da331d5b05c0467f9a0 master_bookmark


-- Show the graph after the sync
  $ mononoke_newadmin changelog -R large-mon graph -i d06c956180c43660142dabd61da09e9c6d2b19a53f43fee62b5f919789e24411,$L_C -M
  o  message: S_C
  │
  o  message: first post-move commit
  │
  o  message: move commit
  │
  o  message: pre-move commit
