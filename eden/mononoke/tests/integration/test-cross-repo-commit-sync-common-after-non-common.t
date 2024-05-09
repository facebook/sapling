# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ export LARGE_REPO_ID=0
  $ export SMALL_REPO_ID=1
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

-- First, push a non common-pushrebase bookmark (other_bookmark) one commit forward to S_C
  $ testtool_drawdag -R small-mon << EOF
  > S_A-S_B-S_C-S_D-S_E-S_F
  > # exists: S_A $S_A
  > # exists: S_B $S_B
  > # bookmark: S_C other_bookmark
  > EOF
  S_A=c74140f562eda7c378d4e8d68e4828239617dd51806f3ccb220433a3ea1a6353
  S_B=1ba347e63a4bf200944c22ade8dbea038dd271ef97af346ba4ccfaaefb10dd4d
  S_C=6899eb0af1d64df45683e6bf22c8b82593b22539dec09394f516f944f6fa8c12
  S_D=542a68bb4fd5a7ba5a047a0bb29a48d660c0ea5114688d00b11658313e8f1e6b
  S_E=a9d1b36d3a6d37d43ff6cd7279e0e02a9f6e1930dc41e1ee129bdfd315572074
  S_F=c8f423b81b6dc422d07144a05bde9fe8ff03a0c7aaf77840418b104125fff9c0

-- Then, push a common pushrebase bookmark two commits forward to S_D
  $ mononoke_newadmin bookmarks -R small-mon set master_bookmark $S_D
  Updating publishing bookmark master_bookmark from 1ba347e63a4bf200944c22ade8dbea038dd271ef97af346ba4ccfaaefb10dd4d to 542a68bb4fd5a7ba5a047a0bb29a48d660c0ea5114688d00b11658313e8f1e6b

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
-- The second bookmark move of master_bookmark to S_D also suceeds and thanks to the fact that there were no competing pushrebases
-- and the date wasn't rewritten there's no divergence between S_C and S_D.
  $ mononoke_newadmin mutable-counters -R large-mon set xreposync_from_1 0
  Value of xreposync_from_1 in repo large-mon(Id: 0) set to 0
  $ with_stripped_logs mononoke_x_repo_sync 1 0 tail --catch-up-once
  Starting session with id * (glob)
  queue size is 3
  processing log entry #1
  0 unsynced ancestors of 1ba347e63a4bf200944c22ade8dbea038dd271ef97af346ba4ccfaaefb10dd4d
  successful sync bookmark update log #1
  processing log entry #2
  1 unsynced ancestors of 6899eb0af1d64df45683e6bf22c8b82593b22539dec09394f516f944f6fa8c12
  syncing 6899eb0af1d64df45683e6bf22c8b82593b22539dec09394f516f944f6fa8c12
  changeset 6899eb0af1d64df45683e6bf22c8b82593b22539dec09394f516f944f6fa8c12 synced as d06c956180c43660142dabd61da09e9c6d2b19a53f43fee62b5f919789e24411 in * (glob)
  successful sync bookmark update log #2
  processing log entry #3
  2 unsynced ancestors of 542a68bb4fd5a7ba5a047a0bb29a48d660c0ea5114688d00b11658313e8f1e6b
  syncing 6899eb0af1d64df45683e6bf22c8b82593b22539dec09394f516f944f6fa8c12 via pushrebase for master_bookmark
  changeset 6899eb0af1d64df45683e6bf22c8b82593b22539dec09394f516f944f6fa8c12 synced as d06c956180c43660142dabd61da09e9c6d2b19a53f43fee62b5f919789e24411 in * (glob)
  syncing 542a68bb4fd5a7ba5a047a0bb29a48d660c0ea5114688d00b11658313e8f1e6b via pushrebase for master_bookmark
  changeset 542a68bb4fd5a7ba5a047a0bb29a48d660c0ea5114688d00b11658313e8f1e6b synced as 3c072c4093381c801d2a575ccc7943e59ece487b455a5f4781ea7c750af2983e in * (glob)
  successful sync bookmark update log #3

-- Show the bookmarks after the sync
  $ mononoke_newadmin bookmarks --repo-name large-mon list
  d06c956180c43660142dabd61da09e9c6d2b19a53f43fee62b5f919789e24411 bookprefix/other_bookmark
  3c072c4093381c801d2a575ccc7943e59ece487b455a5f4781ea7c750af2983e master_bookmark


-- Show the graph after the sync
  $ mononoke_newadmin changelog -R large-mon graph -i d06c956180c43660142dabd61da09e9c6d2b19a53f43fee62b5f919789e24411,3c072c4093381c801d2a575ccc7943e59ece487b455a5f4781ea7c750af2983e -M
  o  message: S_D
  │
  o  message: S_C
  │
  o  message: first post-move commit
  │
  o  message: move commit
  │
  o  message: pre-move commit


-- Sync after both bookmark moves happened
-- This time we inject some commits into large repo simulating direct, unrelated pushes
-- there's no other way to do the sync than to diverge now.
  $ mononoke_newadmin bookmarks -R small-mon set other_bookmark $S_E
  Updating publishing bookmark other_bookmark from 6899eb0af1d64df45683e6bf22c8b82593b22539dec09394f516f944f6fa8c12 to a9d1b36d3a6d37d43ff6cd7279e0e02a9f6e1930dc41e1ee129bdfd315572074
  $ mononoke_x_repo_sync 1 0 tail --catch-up-once  2>&1 | strip_glog
  Starting session with id * (glob)
  queue size is 1
  processing log entry #4
  1 unsynced ancestors of a9d1b36d3a6d37d43ff6cd7279e0e02a9f6e1930dc41e1ee129bdfd315572074
  syncing a9d1b36d3a6d37d43ff6cd7279e0e02a9f6e1930dc41e1ee129bdfd315572074
  changeset a9d1b36d3a6d37d43ff6cd7279e0e02a9f6e1930dc41e1ee129bdfd315572074 synced as 7b854923a6d1a8681ba45d2ea9b704d8f9ac795bfabc393477eb181217745072 in * (glob)
  successful sync bookmark update log #4

  $ testtool_drawdag -R large-mon << EOF
  > S_D-L_A-L_B
  > # exists: S_D 3c072c4093381c801d2a575ccc7943e59ece487b455a5f4781ea7c750af2983e
  > # bookmark: L_B master_bookmark
  > EOF
  L_A=98f43915d8e880b609a40da0ee6c737bf7732283fa19d6e7c796644c63495b0f
  L_B=e0c0d1d403651620aa2d9cbe2f706a4a30f9e910e0986102eb350dcd3300755e
  S_D=3c072c4093381c801d2a575ccc7943e59ece487b455a5f4781ea7c750af2983e

  $ mononoke_newadmin bookmarks -R small-mon set master_bookmark $S_E
  Updating publishing bookmark master_bookmark from 542a68bb4fd5a7ba5a047a0bb29a48d660c0ea5114688d00b11658313e8f1e6b to a9d1b36d3a6d37d43ff6cd7279e0e02a9f6e1930dc41e1ee129bdfd315572074
  $ mononoke_x_repo_sync 1 0 tail --catch-up-once  2>&1 | strip_glog
  Starting session with id * (glob)
  queue size is 1
  processing log entry #5
  1 unsynced ancestors of a9d1b36d3a6d37d43ff6cd7279e0e02a9f6e1930dc41e1ee129bdfd315572074
  syncing a9d1b36d3a6d37d43ff6cd7279e0e02a9f6e1930dc41e1ee129bdfd315572074 via pushrebase for master_bookmark
  changeset a9d1b36d3a6d37d43ff6cd7279e0e02a9f6e1930dc41e1ee129bdfd315572074 synced as 6c69e9c52d3293368e2d26a5e31bed2392ec9d31bd05e4777124d3076e01617e in * (glob)
  successful sync bookmark update log #5

  $ mononoke_newadmin bookmarks --repo-name large-mon list
  7b854923a6d1a8681ba45d2ea9b704d8f9ac795bfabc393477eb181217745072 bookprefix/other_bookmark
  6c69e9c52d3293368e2d26a5e31bed2392ec9d31bd05e4777124d3076e01617e master_bookmark

  $ mononoke_newadmin changelog -R large-mon graph -i 7b854923a6d1a8681ba45d2ea9b704d8f9ac795bfabc393477eb181217745072,6c69e9c52d3293368e2d26a5e31bed2392ec9d31bd05e4777124d3076e01617e -M
  o  message: S_E
  │
  o  message: L_B
  │
  o  message: L_A
  │
  │ o  message: S_E
  ├─╯
  o  message: S_D
  │
  o  message: S_C
  │
  o  message: first post-move commit
  │
  o  message: move commit
  │
  o  message: pre-move commit


-- Sync a change to other bookmark showing how now the choice for a base is tricky as
-- the S_F commit is based on S_E so there are two possible choices here.
  $ mononoke_newadmin bookmarks -R small-mon set other_bookmark $S_F
  Updating publishing bookmark other_bookmark from a9d1b36d3a6d37d43ff6cd7279e0e02a9f6e1930dc41e1ee129bdfd315572074 to c8f423b81b6dc422d07144a05bde9fe8ff03a0c7aaf77840418b104125fff9c0
  $ mononoke_x_repo_sync 1 0 tail --catch-up-once  2>&1 | strip_glog
  * (glob)
  queue size is 1
  processing log entry #6
  1 unsynced ancestors of c8f423b81b6dc422d07144a05bde9fe8ff03a0c7aaf77840418b104125fff9c0
  syncing c8f423b81b6dc422d07144a05bde9fe8ff03a0c7aaf77840418b104125fff9c0
  changeset c8f423b81b6dc422d07144a05bde9fe8ff03a0c7aaf77840418b104125fff9c0 synced as 5c3f0368dead91cb214d9b3983ae632c160960e35ee91ac6f11ad96e5601849d in * (glob)
  successful sync bookmark update log #6
