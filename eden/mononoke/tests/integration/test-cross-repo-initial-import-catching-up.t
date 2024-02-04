# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-xrepo-sync-with-git-submodules.sh"




Setup configuration
  $ run_common_xrepo_sync_with_gitsubmodules_setup

# Test how the initial-import command behaves when it runs again after new
# commits have been added to the small repo.
# EXPECTED: it will only sync the new commits and the ancestry will be correct
# in the large repo.
# NOTE: the initial-import command expects that the commits from the small
# repo HAVE NOT YET BEEN MERGED with the master branch of the large repo.
# After the merge, the live sync command should be used.
Create small repo commits
  $ testtool_drawdag -R "$SMALL_REPO_NAME" --no-default-files <<EOF
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
  $ with_stripped_logs mononoke_x_repo_sync "$SMALL_REPO_ID" "$LARGE_REPO_ID" --log-level=TRACE \
  > initial-import --no-progress-bar -i "$B" --version-name "$LATEST_CONFIG_VERSION_NAME" | rg -v "nitializ"
  enabled stdlog with level: Error (set RUST_LOG to configure)
  Starting session with id * (glob)
  Reloading redacted config from configerator
  Checking if 2999dcf517994fe94506b62e5a9c54f851abd4c4964f98fdd701c013abd9c0c3 is already synced 1->0
  syncing 2999dcf517994fe94506b62e5a9c54f851abd4c4964f98fdd701c013abd9c0c3
  Found 2 unsynced ancestors
  Unsynced ancestors: [
      ChangesetId(
          Blake2(7e97054c51a17ea2c03cd5184826b6a7556d141d57c5a1641bbd62c0854d1a36),
      ),
      ChangesetId(
          Blake2(2999dcf517994fe94506b62e5a9c54f851abd4c4964f98fdd701c013abd9c0c3),
      ),
  ]
  CommitSyncer{1->0}: unsafe_sync_commit called for 7e97054c51a17ea2c03cd5184826b6a7556d141d57c5a1641bbd62c0854d1a36, with hint: CandidateSelectionHint::Only
  derive changeset_info for 7e97054c51a17ea2c03cd5184826b6a7556d141d57c5a1641bbd62c0854d1a36
  CommitSyncer{1->0}: unsafe_sync_commit called for 2999dcf517994fe94506b62e5a9c54f851abd4c4964f98fdd701c013abd9c0c3, with hint: CandidateSelectionHint::Only
  get_commit_sync_outcome_with_hint called for 1->0, cs 7e97054c51a17ea2c03cd5184826b6a7556d141d57c5a1641bbd62c0854d1a36, hint CandidateSelectionHint::Only
  derive changeset_info for 2999dcf517994fe94506b62e5a9c54f851abd4c4964f98fdd701c013abd9c0c3
  derive skeleton_manifests for 7e97054c51a17ea2c03cd5184826b6a7556d141d57c5a1641bbd62c0854d1a36
  CommitSyncer{1->0}: unsafe_sync_commit called for 2999dcf517994fe94506b62e5a9c54f851abd4c4964f98fdd701c013abd9c0c3, with hint: CandidateSelectionHint::Only
  get_commit_sync_outcome_with_hint called for 1->0, cs 7e97054c51a17ea2c03cd5184826b6a7556d141d57c5a1641bbd62c0854d1a36, hint CandidateSelectionHint::Only
  changeset 2999dcf517994fe94506b62e5a9c54f851abd4c4964f98fdd701c013abd9c0c3 synced as 85776cdc88303208a1cde5c614996a89441d3a9175a6311dda34d178428ba652 in * (glob)
  successful sync of head 2999dcf517994fe94506b62e5a9c54f851abd4c4964f98fdd701c013abd9c0c3


  $ clone_and_log_large_repo "85776cdc88303208a1cde5c614996a89441d3a9175a6311dda34d178428ba652"
  o  5e3f6798b6a3 B
  │   smallrepofolder1/bar/c.txt |  1 +
  │   smallrepofolder1/foo/d     |  1 +
  │   2 files changed, 2 insertions(+), 0 deletions(-)
  │
  o  e462fc947f26 A
      smallrepofolder1/bar/b.txt |  1 +
      smallrepofolder1/foo/a.txt |  1 +
      2 files changed, 2 insertions(+), 0 deletions(-)
  
  
  
  Running mononoke_admin to verify mapping
  
  RewrittenAs([(ChangesetId(Blake2(2999dcf517994fe94506b62e5a9c54f851abd4c4964f98fdd701c013abd9c0c3)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
  
  Deriving all the enabled derived data types

Add more commits to small repo
  $ testtool_drawdag -R "$SMALL_REPO_NAME" --no-default-files <<EOF
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
  $ with_stripped_logs mononoke_x_repo_sync "$SMALL_REPO_ID" "$LARGE_REPO_ID" --log-level=TRACE \
  > initial-import --no-progress-bar -i "$D" --version-name "$LATEST_CONFIG_VERSION_NAME" | rg -v "nitializ"
  enabled stdlog with level: Error (set RUST_LOG to configure)
  Starting session with id * (glob)
  Reloading redacted config from configerator
  Checking if d2ba11302a912b679610fd60d7e56dd8f01372c130faa3ae72816d5568b25f3a is already synced 1->0
  syncing d2ba11302a912b679610fd60d7e56dd8f01372c130faa3ae72816d5568b25f3a
  Found 2 unsynced ancestors
  Unsynced ancestors: [
      ChangesetId(
          Blake2(9eeb57261a4dfbeeb2e1c06ef6dc3f83b11e314eb34c598f2d042967b1938583),
      ),
      ChangesetId(
          Blake2(d2ba11302a912b679610fd60d7e56dd8f01372c130faa3ae72816d5568b25f3a),
      ),
  ]
  CommitSyncer{1->0}: unsafe_sync_commit called for 9eeb57261a4dfbeeb2e1c06ef6dc3f83b11e314eb34c598f2d042967b1938583, with hint: CandidateSelectionHint::Only
  get_commit_sync_outcome_with_hint called for 1->0, cs 2999dcf517994fe94506b62e5a9c54f851abd4c4964f98fdd701c013abd9c0c3, hint CandidateSelectionHint::Only
  derive changeset_info for 9eeb57261a4dfbeeb2e1c06ef6dc3f83b11e314eb34c598f2d042967b1938583
  derive skeleton_manifests for 2999dcf517994fe94506b62e5a9c54f851abd4c4964f98fdd701c013abd9c0c3
  CommitSyncer{1->0}: unsafe_sync_commit called for d2ba11302a912b679610fd60d7e56dd8f01372c130faa3ae72816d5568b25f3a, with hint: CandidateSelectionHint::Only
  get_commit_sync_outcome_with_hint called for 1->0, cs 9eeb57261a4dfbeeb2e1c06ef6dc3f83b11e314eb34c598f2d042967b1938583, hint CandidateSelectionHint::Only
  derive changeset_info for d2ba11302a912b679610fd60d7e56dd8f01372c130faa3ae72816d5568b25f3a
  derive skeleton_manifests for 9eeb57261a4dfbeeb2e1c06ef6dc3f83b11e314eb34c598f2d042967b1938583
  CommitSyncer{1->0}: unsafe_sync_commit called for d2ba11302a912b679610fd60d7e56dd8f01372c130faa3ae72816d5568b25f3a, with hint: CandidateSelectionHint::Only
  get_commit_sync_outcome_with_hint called for 1->0, cs 9eeb57261a4dfbeeb2e1c06ef6dc3f83b11e314eb34c598f2d042967b1938583, hint CandidateSelectionHint::Only
  changeset d2ba11302a912b679610fd60d7e56dd8f01372c130faa3ae72816d5568b25f3a synced as ccfdf094e4710a77de7b36c4324fa7ee64dafba4067726e383db62273553466b in * (glob)
  successful sync of head d2ba11302a912b679610fd60d7e56dd8f01372c130faa3ae72816d5568b25f3a

  $ clone_and_log_large_repo "ccfdf094e4710a77de7b36c4324fa7ee64dafba4067726e383db62273553466b"
  abort: destination 'large_repo' is not empty
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
  
  
  
  Running mononoke_admin to verify mapping
  
  RewrittenAs([(ChangesetId(Blake2(d2ba11302a912b679610fd60d7e56dd8f01372c130faa3ae72816d5568b25f3a)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
  
  Deriving all the enabled derived data types
