# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/cross_repo/library-git-submodules-config-setup.sh"
  $ . "${TEST_FIXTURES}/cross_repo/library-git-submodules-helpers.sh"




Setup configuration
  $ quiet run_common_xrepo_sync_with_gitsubmodules_setup

# Test how the initial-import command behaves when it runs again after new
# commits have been added to the small repo.
# EXPECTED: it will only sync the new commits and the ancestry will be correct
# in the large repo.
# NOTE: the initial-import command expects that the commits from the small
# repo HAVE NOT YET BEEN MERGED with the master branch of the large repo.
# After the merge, the live sync command should be used.
Create small repo commits
  $ testtool_drawdag -R "$SUBMODULE_REPO_NAME" --no-default-files <<EOF
  > A-B
  > # modify: A "foo/a.txt" "creating foo directory"
  > # modify: A "bar/b.txt" "creating bar directory"
  > # modify: B "bar/c.txt" "random change"
  > # modify: B "foo/d" "another random change"
  > # bookmark: B master
  > EOF
  A=7e97054c51a17ea2c03cd5184826b6a7556d141d57c5a1641bbd62c0854d1a36
  B=2999dcf517994fe94506b62e5a9c54f851abd4c4964f98fdd701c013abd9c0c3

# Ignoring lines with `initializing` or `initialized
  $ mononoke_x_repo_sync "$SUBMODULE_REPO_ID" "$LARGE_REPO_ID" --log-level=DEBUG \
  > initial-import --no-progress-bar --derivation-batch-size 2 -i "$B" --version-name "$LATEST_CONFIG_VERSION_NAME" 2>&1 | \
  > rg -v "nitializ" | rg -v "derive" | rg -v "Upload" | tee $TESTTMP/initial_import.out
  [INFO] Starting session with id * (glob)
  [INFO] Starting up X Repo Sync from small repo small_repo to large repo large_repo
  [INFO] Checking if 2999dcf517994fe94506b62e5a9c54f851abd4c4964f98fdd701c013abd9c0c3 is already synced 11->10
  [INFO] Syncing 2999dcf517994fe94506b62e5a9c54f851abd4c4964f98fdd701c013abd9c0c3 for initial import
  [INFO] Source repo: small_repo / Target repo: large_repo
  [DEBUG] Automatic derivation is enabled
  [INFO] Found 2 unsynced ancestors
  [DEBUG] CommitSyncData{11->10}: unsafe_sync_commit called for 7e97054c51a17ea2c03cd5184826b6a7556d141d57c5a1641bbd62c0854d1a36, with hint: CandidateSelectionHint::Only
  [DEBUG] Ancestor 7e97054c51a17ea2c03cd5184826b6a7556d141d57c5a1641bbd62c0854d1a36 synced successfully as ac220d3e57adf7c31a869141787d3bc638d79a3f1dd54b0ba54d545c260f14e6
  [DEBUG] CommitSyncData{11->10}: unsafe_sync_commit called for 2999dcf517994fe94506b62e5a9c54f851abd4c4964f98fdd701c013abd9c0c3, with hint: CandidateSelectionHint::Only
  [DEBUG] get_commit_sync_outcome_with_hint called for 11->10, cs 7e97054c51a17ea2c03cd5184826b6a7556d141d57c5a1641bbd62c0854d1a36, hint CandidateSelectionHint::Only
  [DEBUG] Ancestor 2999dcf517994fe94506b62e5a9c54f851abd4c4964f98fdd701c013abd9c0c3 synced successfully as 85776cdc88303208a1cde5c614996a89441d3a9175a6311dda34d178428ba652
  [DEBUG] Finished bulk derivation of 2 changesets
  [DEBUG] CommitSyncData{11->10}: unsafe_sync_commit called for 2999dcf517994fe94506b62e5a9c54f851abd4c4964f98fdd701c013abd9c0c3, with hint: CandidateSelectionHint::Only
  [DEBUG] get_commit_sync_outcome_with_hint called for 11->10, cs 7e97054c51a17ea2c03cd5184826b6a7556d141d57c5a1641bbd62c0854d1a36, hint CandidateSelectionHint::Only
  [INFO] changeset 2999dcf517994fe94506b62e5a9c54f851abd4c4964f98fdd701c013abd9c0c3 synced as 85776cdc88303208a1cde5c614996a89441d3a9175a6311dda34d178428ba652 * (glob)
  [INFO] successful sync of head 2999dcf517994fe94506b62e5a9c54f851abd4c4964f98fdd701c013abd9c0c3
  [INFO] X Repo Sync execution finished from small repo small_repo to large repo large_repo


  $ SYNCED_HEAD=$(rg ".+synced as (\w+) .+" -or '$1' "$TESTTMP/initial_import.out")
  $ clone_and_log_large_repo "$SYNCED_HEAD"
  o  5e3f6798b6a3 B
  │   smallrepofolder1/bar/c.txt |  1 +
  │   smallrepofolder1/foo/d     |  1 +
  │   2 files changed, 2 insertions(+), 0 deletions(-)
  │
  o  e462fc947f26 A
      smallrepofolder1/bar/b.txt |  1 +
      smallrepofolder1/foo/a.txt |  1 +
      2 files changed, 2 insertions(+), 0 deletions(-)
  
  @  54a6db91baf1 L_A
      file_in_large_repo.txt |  1 +
      1 files changed, 1 insertions(+), 0 deletions(-)
  
  
  
  Running mononoke_admin to verify mapping
  
  RewrittenAs([(ChangesetId(Blake2(2999dcf517994fe94506b62e5a9c54f851abd4c4964f98fdd701c013abd9c0c3)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
  
  Deriving all the enabled derived data types






Add more commits to small repo
  $ testtool_drawdag -R "$SUBMODULE_REPO_NAME" --no-default-files <<EOF
  > B-C-D
  > # exists: B $B
  > # modify: C "bar/b.txt" "more changes"
  > # modify: D "bar/c.txt" "more changes"
  > # bookmark: D master
  > EOF
  B=2999dcf517994fe94506b62e5a9c54f851abd4c4964f98fdd701c013abd9c0c3
  C=9eeb57261a4dfbeeb2e1c06ef6dc3f83b11e314eb34c598f2d042967b1938583
  D=d2ba11302a912b679610fd60d7e56dd8f01372c130faa3ae72816d5568b25f3a



# Ignoring lines with `initializing` or `initialized
  $ mononoke_x_repo_sync "$SUBMODULE_REPO_ID" "$LARGE_REPO_ID" --log-level=DEBUG \
  > initial-import --no-progress-bar --derivation-batch-size 2 -i "$D" --version-name "$LATEST_CONFIG_VERSION_NAME" 2>&1 | \
  > rg -v "nitializ" | rg -v "derive" | rg -v "Upload"
  [INFO] Starting session with id * (glob)
  [INFO] Starting up X Repo Sync from small repo small_repo to large repo large_repo
  [INFO] Checking if d2ba11302a912b679610fd60d7e56dd8f01372c130faa3ae72816d5568b25f3a is already synced 11->10
  [INFO] Syncing d2ba11302a912b679610fd60d7e56dd8f01372c130faa3ae72816d5568b25f3a for initial import
  [INFO] Source repo: small_repo / Target repo: large_repo
  [DEBUG] Automatic derivation is enabled
  [INFO] Found 2 unsynced ancestors
  [DEBUG] CommitSyncData{11->10}: unsafe_sync_commit called for 9eeb57261a4dfbeeb2e1c06ef6dc3f83b11e314eb34c598f2d042967b1938583, with hint: CandidateSelectionHint::Only
  [DEBUG] get_commit_sync_outcome_with_hint called for 11->10, cs 2999dcf517994fe94506b62e5a9c54f851abd4c4964f98fdd701c013abd9c0c3, hint CandidateSelectionHint::Only
  [DEBUG] Ancestor 9eeb57261a4dfbeeb2e1c06ef6dc3f83b11e314eb34c598f2d042967b1938583 synced successfully as eee07cc327b80fd172bbbe2933615d1f4685a3a032eed0fc52c02c01e8f49c42
  [DEBUG] CommitSyncData{11->10}: unsafe_sync_commit called for d2ba11302a912b679610fd60d7e56dd8f01372c130faa3ae72816d5568b25f3a, with hint: CandidateSelectionHint::Only
  [DEBUG] get_commit_sync_outcome_with_hint called for 11->10, cs 9eeb57261a4dfbeeb2e1c06ef6dc3f83b11e314eb34c598f2d042967b1938583, hint CandidateSelectionHint::Only
  [DEBUG] Ancestor d2ba11302a912b679610fd60d7e56dd8f01372c130faa3ae72816d5568b25f3a synced successfully as ccfdf094e4710a77de7b36c4324fa7ee64dafba4067726e383db62273553466b
  [DEBUG] Finished bulk derivation of 2 changesets
  [DEBUG] CommitSyncData{11->10}: unsafe_sync_commit called for d2ba11302a912b679610fd60d7e56dd8f01372c130faa3ae72816d5568b25f3a, with hint: CandidateSelectionHint::Only
  [DEBUG] get_commit_sync_outcome_with_hint called for 11->10, cs 9eeb57261a4dfbeeb2e1c06ef6dc3f83b11e314eb34c598f2d042967b1938583, hint CandidateSelectionHint::Only
  [INFO] changeset d2ba11302a912b679610fd60d7e56dd8f01372c130faa3ae72816d5568b25f3a synced as ccfdf094e4710a77de7b36c4324fa7ee64dafba4067726e383db62273553466b in * (glob)
  [INFO] successful sync of head d2ba11302a912b679610fd60d7e56dd8f01372c130faa3ae72816d5568b25f3a
  [INFO] X Repo Sync execution finished from small repo small_repo to large repo large_repo


  $ clone_and_log_large_repo "ccfdf094e4710a77de7b36c4324fa7ee64dafba4067726e383db62273553466b"
  o  71fdac6141e7 D
  │   smallrepofolder1/bar/c.txt |  2 +-
  │   1 files changed, 1 insertions(+), 1 deletions(-)
  │
  o  368fd13402ee C
  │   smallrepofolder1/bar/b.txt |  2 +-
  │   1 files changed, 1 insertions(+), 1 deletions(-)
  │
  o  5e3f6798b6a3 B
  │   smallrepofolder1/bar/c.txt |  1 +
  │   smallrepofolder1/foo/d     |  1 +
  │   2 files changed, 2 insertions(+), 0 deletions(-)
  │
  o  e462fc947f26 A
      smallrepofolder1/bar/b.txt |  1 +
      smallrepofolder1/foo/a.txt |  1 +
      2 files changed, 2 insertions(+), 0 deletions(-)
  
  @  54a6db91baf1 L_A
      file_in_large_repo.txt |  1 +
      1 files changed, 1 insertions(+), 0 deletions(-)
  
  
  
  Running mononoke_admin to verify mapping
  
  RewrittenAs([(ChangesetId(Blake2(d2ba11302a912b679610fd60d7e56dd8f01372c130faa3ae72816d5568b25f3a)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
  
  Deriving all the enabled derived data types
