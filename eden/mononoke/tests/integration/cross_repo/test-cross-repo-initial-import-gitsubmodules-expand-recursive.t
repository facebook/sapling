# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# This test will set up 3 git repos: A, B and C
# A will depend on B as a submodule and B will depend on C.
#

# The test will run an initial-import and set up a live sync from A to a large
# repo, expanding the git submodule changes.
# All files from all submodules need to be copied in A, in the appropriate
# subdirectory.
# After that, we make more changes to the submodules, update their git repos,
# import the new commits and run the forward syncer again, to test the workflow
# one more time.

  $ export ENABLE_BOOKMARK_CACHE=1

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"
  $ . "${TEST_FIXTURES}/cross_repo/library-git-submodules-repos-setup.sh"



Run the x-repo with submodules setup

  $ quiet run_common_xrepo_sync_with_gitsubmodules_setup
  $ set_git_submodules_action_in_config_version "$LATEST_CONFIG_VERSION_NAME" "$SUBMODULE_REPO_ID" 3 # 3=expand
  $ set_git_submodule_dependencies_in_config_version "$LATEST_CONFIG_VERSION_NAME" \
  > "$SUBMODULE_REPO_ID" "{\"git-repo-b\": $REPO_B_ID, \"git-repo-b/git-repo-c\": $REPO_C_ID, \"repo_c\": $REPO_C_ID}"
  $ killandwait $MONONOKE_PID
  $ start_and_wait_for_mononoke_server

  $ REPO_A_FOLDER="smallrepofolder1" quiet setup_all_repos_for_test

  $ mononoke_admin bookmarks -R "$SUBMODULE_REPO_NAME" list -S hg
  heads/master_bookmark


  $ QUIET_LOGGING_LOG_FILE="$TESTTMP/xrepo_sync_last_logs.out" wait_for_xrepo_sync 2

  $ cd "$TESTTMP/$LARGE_REPO_NAME"
  $ wait_for_bookmark_move_away_edenapi large_repo master_bookmark $(hg whereami)
  $ hg pull -q
  $ hg co -q master_bookmark

  $ hg log --graph -T '{node} {desc}\n' -r "all()"
  @  d246b01a5a5baff205958295aa764916ae288291 Remove repo C submodule from repo A
  │
  o  d3dae76d4349c88c24d60fe533bd9fbd02ddd5ae Update submodule B in repo A
  │
  o  ada44b220ff885a5757bf80bee03e64f0b0e063d Change directly in A
  │
  o  e2b260a2b04f485be16d9a59594dce5f2b652ea2 Added git repo C as submodule directly in A
  │
  o    c0240984981f6f70094e0cd4f42d1e33c4c86a69 [MEGAREPO GRADUAL MERGE] gradual merge (7)
  ├─╮
  │ o    eb7057489fd5d07098a7dce76303fb661f9ff21b [MEGAREPO GRADUAL MERGE] gradual merge (6)
  │ ├─╮
  │ │ o    67ce5c45ccfe824e860656b64370092aa899329a [MEGAREPO GRADUAL MERGE] gradual merge (5)
  │ │ ├─╮
  │ │ │ o    3d5d1c2ca8842a63492b00b98510a9f6c641136c [MEGAREPO GRADUAL MERGE] gradual merge (4)
  │ │ │ ├─╮
  │ │ │ │ o    19d222a5612c8212fb1d5c9ba457254e9bb8c94c [MEGAREPO GRADUAL MERGE] gradual merge (3)
  │ │ │ │ ├─╮
  │ │ │ │ │ o    b019b0d36eb2dd282ce738ec13fdbf3be30a77d4 [MEGAREPO GRADUAL MERGE] gradual merge (2)
  │ │ │ │ │ ├─╮
  │ │ │ │ │ │ o    c40e45f2cc7cf814695c23f13cf7e07a5fda1545 [MEGAREPO GRADUAL MERGE] gradual merge (1)
  │ │ │ │ │ │ ├─╮
  │ │ │ │ │ │ │ o    aea509f84730e4e33ee5818d074ef07ab2e84767 [MEGAREPO GRADUAL MERGE] gradual merge (0)
  │ │ │ │ │ │ │ ├─╮
  │ │ │ │ │ │ │ │ o  10dab983a27fce66a4c9852d40c4fd36618d63a7 [MEGAREPO DELETE] deletion commits for merge into large repo (6)
  │ │ │ │ │ │ ├───╯
  │ │ │ │ │ │ o │  9f34257829fbf29611c4bdc4b4e48c993c72d2e6 [MEGAREPO DELETE] deletion commits for merge into large repo (5)
  │ │ │ │ │ ├─╯ │
  │ │ │ │ │ o   │  b3109b39500ffcbb09a22bea594d32957e28b0e3 [MEGAREPO DELETE] deletion commits for merge into large repo (4)
  │ │ │ │ ├─╯   │
  │ │ │ │ o     │  43f727449960cc7effbf84da6e54a6daf4f77d99 [MEGAREPO DELETE] deletion commits for merge into large repo (3)
  │ │ │ ├─╯     │
  │ │ │ o       │  9d59171d496f660ee0276013e446d5687b69394f [MEGAREPO DELETE] deletion commits for merge into large repo (2)
  │ │ ├─╯       │
  │ │ o         │  5d6979a70f2b49a7fe30cabdbb771804bec798ae [MEGAREPO DELETE] deletion commits for merge into large repo (1)
  │ ├─╯         │
  │ o           │  c1f01db6a93222463fad3133b5eb89809d414cde [MEGAREPO DELETE] deletion commits for merge into large repo (0)
  ├─╯           │
  o             │  1f9d3769f8c22b50db3ed0105c9d0e9490bbe7e9 Added git repo B as submodule in A
  │             │
  o             │  e2c69ce8cc11691984e50e6023f4bbf4271aa4c3 Add regular_dir/aardvar
  │             │
  o             │  df9086c771290c305c738040313bf1cc5759eba9 Add root_file
                │
                o  54a6db91baf1c10921369339b50e5a174a7ca82e L_A
  

-- Check that deletions were made properly, i.e. submodule in repo_c was entirely
-- deleted and the files deleted in repo B were deleted inside its copy.
  $ hg show --stat -T 'commit: {node}\n{desc}\n' .
  commit: d246b01a5a5baff205958295aa764916ae288291
  Remove repo C submodule from repo A
   smallrepofolder1/.gitmodules              |  3 ---
   smallrepofolder1/.x-repo-submodule-repo_c |  1 -
   smallrepofolder1/repo_c/choo              |  1 -
   smallrepofolder1/repo_c/choo3             |  1 -
   smallrepofolder1/repo_c/choo4             |  1 -
   smallrepofolder1/repo_c/hoo/qux           |  1 -
   6 files changed, 0 insertions(+), 8 deletions(-)
  


TODO(T174902563): Fix deletion of submodules in EXPAND submodule action.
  $ tree -a -I ".hg" &> ${TESTTMP}/large_repo_tree_2
  $ diff -y -t -T ${TESTTMP}/large_repo_tree_1 ${TESTTMP}/large_repo_tree_2
  .                                                                  .
  |-- file_in_large_repo.txt                                         |-- file_in_large_repo.txt
  `-- smallrepofolder1                                               `-- smallrepofolder1
      |-- .gitmodules                                                    |-- .gitmodules
      |-- .x-repo-submodule-git-repo-b                                   |-- .x-repo-submodule-git-repo-b
      |-- .x-repo-submodule-repo_c                                <
      |-- duplicates                                                     |-- duplicates
      |   |-- x                                                          |   |-- x
      |   |-- y                                                          |   |-- y
      |   `-- z                                                          |   `-- z
      |-- git-repo-b                                                     |-- git-repo-b
      |   |-- .gitmodules                                                |   |-- .gitmodules
      |   |-- .x-repo-submodule-git-repo-c                               |   |-- .x-repo-submodule-git-repo-c
      |   |-- bar                                                 <
      |   |   `-- zoo                                             <
      |   |-- foo                                                 <
      |   `-- git-repo-c                                                 |   `-- git-repo-c
      |       |-- choo                                                   |       |-- choo
                                                                  >      |       |-- choo3
                                                                  >      |       |-- choo4
      |       `-- hoo                                                    |       `-- hoo
      |           `-- qux                                                |           `-- qux
      |-- regular_dir                                                    |-- regular_dir
      |   `-- aardvar                                                    |   `-- aardvar
      |-- repo_c                                                  <
      |   |-- choo                                                <
      |   `-- hoo                                                 <
      |       `-- qux                                             <
      `-- root_file                                                      `-- root_file
  
  9 directories, 17 files                                         |  6 directories, 14 files
  [1]

-- Check that the diff that updates the submodule generates the correct delta
-- (i.e. instead of copying the entire working copy of the submodule every time)
  $ hg show --stat -T 'commit: {node}\n{desc}\n' .^
  commit: d3dae76d4349c88c24d60fe533bd9fbd02ddd5ae
  Update submodule B in repo A
   smallrepofolder1/.x-repo-submodule-git-repo-b            |  2 +-
   smallrepofolder1/.x-repo-submodule-repo_c                |  2 +-
   smallrepofolder1/git-repo-b/.x-repo-submodule-git-repo-c |  2 +-
   smallrepofolder1/git-repo-b/bar/zoo                      |  1 -
   smallrepofolder1/git-repo-b/foo                          |  1 -
   smallrepofolder1/git-repo-b/git-repo-c/choo3             |  1 +
   smallrepofolder1/git-repo-b/git-repo-c/choo4             |  1 +
   smallrepofolder1/repo_c/choo3                            |  1 +
   smallrepofolder1/repo_c/choo4                            |  1 +
   9 files changed, 7 insertions(+), 5 deletions(-)
  
  $ cat smallrepofolder1/.x-repo-submodule-git-repo-b
  0597690a839ce11a250139dae33ee85d9772a47a (no-eol)

-- Also check that our two binaries that can verify working copy are able to deal with expansions
  $ REPOIDLARGE=$LARGE_REPO_ID REPOIDSMALL=$SUBMODULE_REPO_ID verify_wc $(hg log -r master_bookmark -T '{node}')

-- The check-push-redirection-prereqs should behave the same both ways but let's verify it (we had bugs where it didn't)
-- (those outputs are still not correct but that's expected)
  $ quiet_grep "all is well" -- mononoke_admin megarepo check-prereqs --source-repo-id $SUBMODULE_REPO_ID --target-repo-id $LARGE_REPO_ID --source-changeset bm=heads/master_bookmark --target-changeset bm=master_bookmark --version "$LATEST_CONFIG_VERSION_NAME" | tee $TESTTMP/push_redir_prereqs_small_large
  [INFO] all is well!

  $ quiet_grep "all is well" -- mononoke_admin megarepo check-prereqs --source-repo-id $LARGE_REPO_ID --target-repo-id $SUBMODULE_REPO_ID --source-changeset bm=master_bookmark --target-changeset bm=heads/master_bookmark --version "$LATEST_CONFIG_VERSION_NAME" | tee $TESTTMP/push_redir_prereqs_large_small
  [INFO] all is well!
  $ diff -wbBdu $TESTTMP/push_redir_prereqs_small_large $TESTTMP/push_redir_prereqs_large_small

-- Let's corrupt the expansion and check if validation complains
-- (those outputs are still not correct but that's expected)
  $ echo corrupt > smallrepofolder1/git-repo-b/git-repo-c/choo3
  $ echo corrupt > smallrepofolder1/.x-repo-submodule-git-repo-b
  $ hg commit -m "submodule corruption"
  $ hg push -q --to master_bookmark
  $ quiet_grep "mismatch" -- mononoke_admin megarepo check-prereqs --source-repo-id $SUBMODULE_REPO_ID --target-repo-id $LARGE_REPO_ID --source-changeset bm=heads/master_bookmark --target-changeset bm=master_bookmark  --version "$LATEST_CONFIG_VERSION_NAME" | tee $TESTTMP/push_redir_prereqs_small_large
  submodule expansion mismatch: Failed to fetch content from content id 06a434694d9172d617062abd92f015f73978fb17dd6bcc54e708cd2c6f247970 file containing the submodule's git commit hash

  $ quiet_grep "mismatch" -- mononoke_admin megarepo check-prereqs --source-repo-id $LARGE_REPO_ID --target-repo-id $SUBMODULE_REPO_ID --source-changeset bm=master_bookmark --target-changeset bm=heads/master_bookmark  --version "$LATEST_CONFIG_VERSION_NAME" | sort | tee $TESTTMP/push_redir_prereqs_large_small
  submodule expansion mismatch: Failed to fetch content from content id 06a434694d9172d617062abd92f015f73978fb17dd6bcc54e708cd2c6f247970 file containing the submodule's git commit hash

  $ diff -wbBdu $TESTTMP/push_redir_prereqs_small_large $TESTTMP/push_redir_prereqs_large_small

-- ------------------------------------------------------------------------------
-- Test hg xrepo lookup with commits that are synced

-- Helper function to look for the mapping in the database using admin and then
-- call hgedenpi committranslateids endpoint from large to small.
  $ function check_mapping_and_run_xrepo_lookup_large_to_small {
  >   local hg_hash=$1; shift;
  >   
  >   printf "Check mapping in database with Mononoke admin\n"
  >   mononoke_admin \
  >     cross-repo --source-repo-id $LARGE_REPO_ID --target-repo-id $SUBMODULE_REPO_ID map -i $hg_hash | rg -v "using repo"
  >   printf "\n\nCall hg committranslateids\n"
  >   hg debugapi -e committranslateids \
  >     -i "[{'Hg': '$hg_hash'}]" -i "'Bonsai'" -i None -i "'$SUBMODULE_REPO_NAME'"
  >   
  > }


-- Looking up synced commits from large to small.
-- EXPECT: all of them should return the same value as mapping check using admin

-- Commit: Change directly in A
  $ check_mapping_and_run_xrepo_lookup_large_to_small ada44b220ff885a5757bf80bee03e64f0b0e063d
  Check mapping in database with Mononoke admin
  RewrittenAs([(ChangesetId(Blake2(4aee0499ea629ebcd9d0e4be89267d7a4eab5e72f988c20a392d59081db0c32a)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
  
  
  Call hg committranslateids
  [{"commit": {"Hg": bin("ada44b220ff885a5757bf80bee03e64f0b0e063d")},
    "translated": {"Bonsai": bin("4aee0499ea629ebcd9d0e4be89267d7a4eab5e72f988c20a392d59081db0c32a")}}]

-- Commit: Update submodule B in repo A
  $ check_mapping_and_run_xrepo_lookup_large_to_small d3dae76d4349c88c24d60fe533bd9fbd02ddd5ae
  Check mapping in database with Mononoke admin
  RewrittenAs([(ChangesetId(Blake2(b86f7426fc1fe95e22b6bef591e7ba9c8385b86f7b85abd3a377f941d39522af)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
  
  
  Call hg committranslateids
  [{"commit": {"Hg": bin("d3dae76d4349c88c24d60fe533bd9fbd02ddd5ae")},
    "translated": {"Bonsai": bin("b86f7426fc1fe95e22b6bef591e7ba9c8385b86f7b85abd3a377f941d39522af")}}]

-- Check an original commit from small repo (before merge)
-- Commit: Add regular_dir/aardvar
  $ check_mapping_and_run_xrepo_lookup_large_to_small e2c69ce8cc11691984e50e6023f4bbf4271aa4c3
  Check mapping in database with Mononoke admin
  RewrittenAs([(ChangesetId(Blake2(856b09638e2550d912282c5a9e8bd47fdf1a899545f9f4a05430a8dc7be1f768)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
  
  
  Call hg committranslateids
  [{"commit": {"Hg": bin("e2c69ce8cc11691984e50e6023f4bbf4271aa4c3")},
    "translated": {"Bonsai": bin("856b09638e2550d912282c5a9e8bd47fdf1a899545f9f4a05430a8dc7be1f768")}}]


-- ------------------------------------------------------------------------------
-- Test backsyncing (i.e. large to small)

  $ cd "$TESTTMP/$LARGE_REPO_NAME" || exit
  $ hg pull -q && hg co -q master_bookmark
  $ hg status
  $ hg co -q .^ # go before the commit that corrupts submodules
  $ hg status
  $ enable commitcloud infinitepush # to push commits to server
  $ function hg_log() {
  >   hg log --graph -T '{node|short} {desc}\n' "$@"
  > }

  $ hg_log
  o  cd7933d8ab7a submodule corruption
  │
  @  d246b01a5a5b Remove repo C submodule from repo A
  │
  o  d3dae76d4349 Update submodule B in repo A
  │
  o  ada44b220ff8 Change directly in A
  │
  o  e2b260a2b04f Added git repo C as submodule directly in A
  │
  o    c0240984981f [MEGAREPO GRADUAL MERGE] gradual merge (7)
  ├─╮
  │ o    eb7057489fd5 [MEGAREPO GRADUAL MERGE] gradual merge (6)
  │ ├─╮
  │ │ o    67ce5c45ccfe [MEGAREPO GRADUAL MERGE] gradual merge (5)
  │ │ ├─╮
  │ │ │ o    3d5d1c2ca884 [MEGAREPO GRADUAL MERGE] gradual merge (4)
  │ │ │ ├─╮
  │ │ │ │ o    19d222a5612c [MEGAREPO GRADUAL MERGE] gradual merge (3)
  │ │ │ │ ├─╮
  │ │ │ │ │ o    b019b0d36eb2 [MEGAREPO GRADUAL MERGE] gradual merge (2)
  │ │ │ │ │ ├─╮
  │ │ │ │ │ │ o    c40e45f2cc7c [MEGAREPO GRADUAL MERGE] gradual merge (1)
  │ │ │ │ │ │ ├─╮
  │ │ │ │ │ │ │ o    aea509f84730 [MEGAREPO GRADUAL MERGE] gradual merge (0)
  │ │ │ │ │ │ │ ├─╮
  │ │ │ │ │ │ │ │ o  10dab983a27f [MEGAREPO DELETE] deletion commits for merge into large repo (6)
  │ │ │ │ │ │ ├───╯
  │ │ │ │ │ │ o │  9f34257829fb [MEGAREPO DELETE] deletion commits for merge into large repo (5)
  │ │ │ │ │ ├─╯ │
  │ │ │ │ │ o   │  b3109b39500f [MEGAREPO DELETE] deletion commits for merge into large repo (4)
  │ │ │ │ ├─╯   │
  │ │ │ │ o     │  43f727449960 [MEGAREPO DELETE] deletion commits for merge into large repo (3)
  │ │ │ ├─╯     │
  │ │ │ o       │  9d59171d496f [MEGAREPO DELETE] deletion commits for merge into large repo (2)
  │ │ ├─╯       │
  │ │ o         │  5d6979a70f2b [MEGAREPO DELETE] deletion commits for merge into large repo (1)
  │ ├─╯         │
  │ o           │  c1f01db6a932 [MEGAREPO DELETE] deletion commits for merge into large repo (0)
  ├─╯           │
  o             │  1f9d3769f8c2 Added git repo B as submodule in A
  │             │
  o             │  e2c69ce8cc11 Add regular_dir/aardvar
  │             │
  o             │  df9086c77129 Add root_file
                │
                o  54a6db91baf1 L_A
  

  $ tree
  .
  |-- file_in_large_repo.txt
  `-- smallrepofolder1
      |-- duplicates
      |   |-- x
      |   |-- y
      |   `-- z
      |-- git-repo-b
      |   `-- git-repo-c
      |       |-- choo
      |       |-- choo3
      |       |-- choo4
      |       `-- hoo
      |           `-- qux
      |-- regular_dir
      |   `-- aardvar
      `-- root_file
  
  6 directories, 10 files
  $ function backsync_get_info_and_derive_data() {
  >   hg cloud backup -q
  >   COMMIT_TO_SYNC=$(hg whereami)
  >   COMMIT_TITLE=$(hg log -l1  -T "{truncate(desc, 1)}")
  >   printf "Processing commit: $COMMIT_TITLE\n"
  >   printf "Commit hash: $COMMIT_TO_SYNC\n"
  >   
  >   (check_mapping_and_run_xrepo_lookup_large_to_small \
  >     $COMMIT_TO_SYNC && echo "Success!") 2>&1 | tee $TESTTMP/lookup_commit \
  >     | rg "error|Success" || true;
  >   
  >   # Return early if sync fails
  >   SYNC_EXIT_CODE=${PIPESTATUS[0]}
  >   if [ $SYNC_EXIT_CODE -ne 0 ]; then return $SYNC_EXIT_CODE; fi
  >   SYNCED_BONSAI=$(rg '"translated": \{"Bonsai": bin\("(\w+)"\)\}\}\]' -or '$1' $TESTTMP/lookup_commit);
  >   
  >   printf "\n\nSubmodule repo commit info using admin:\n"
  >   mononoke_admin fetch -R "$SUBMODULE_REPO_NAME" -i "$SYNCED_BONSAI" \
  >     | rg -v "Author"
  > 
  >   printf "\n\nDeriving all enabled types except hgchangesets and filenodes\n";
  >   (mononoke_admin derived-data -R "$SUBMODULE_REPO_NAME" derive -i $SYNCED_BONSAI \
  >     -T fsnodes -T unodes -T fastlog -T fsnodes -T blame -T changeset_info \
  >     -T skeleton_manifests -T deleted_manifest -T bssm_v3 \
  >     -T git_commits -T git_delta_manifests_v2 \
  >       && echo "Success!") 2>&1 | rg "Error|Success" || true;
  > }

-- Change a large repo file and try to backsync it to small repo
-- EXPECT: commit isn't synced and returns working copy equivalent instead
  $ echo "changing large repo file" > file_in_large_repo.txt
  $ hg commit -A -m "Changing large repo file"
  $ hg push -q -r . --to master_bookmark --non-forward-move --pushvar NON_FAST_FORWARD=true
  $ backsync_get_info_and_derive_data
  Processing commit: Changing large repo file
  Commit hash: 48021e7aeafd324f9976f551aea60aa88dd9f61a
  Success!
  
  
  Submodule repo commit info using admin:
  BonsaiChangesetId: de0a58fea04aaf7e162bcb87017752be9d3c838525df6d75a0b897ffaa068a28
  Message: Remove repo C submodule from repo A
  
  FileChanges:
  	 ADDED/MODIFIED: .gitmodules f98d40341818ca2b4b820319487d7f21ebf2f4ea2b4e2d45bab2100f212f2d49
  	 REMOVED: repo_c
  
  
  
  Deriving all enabled types except hgchangesets and filenodes
  Success!

-- Change a small repo file outside of a submodule expansion
-- EXPECT: commit is backsynced normally because it doesn't touch submodule expansions
  $ echo "changing small repo file" > smallrepofolder1/regular_dir/aardvar
  $ hg commit -A -m "Changing small repo in large repo (not submodule)"
  $ hg push -q -r . --to master_bookmark --non-forward-move --pushvar NON_FAST_FORWARD=true
  $ backsync_get_info_and_derive_data
  Processing commit: Changing small repo in large repo (not submodule)
  Commit hash: 35e70dc7f37c3f51876a0f017a733a13809bef32
  Success!
  
  
  Submodule repo commit info using admin:
  BonsaiChangesetId: 8810bc8cf29ac2dce869da1975b8168a43b8f08232d9e1a9dac52013ac2251e2
  Message: Changing small repo in large repo (not submodule)
  FileChanges:
  	 ADDED/MODIFIED: regular_dir/aardvar 58186314bed8b207f5f63a4a58aa858e715f25225a6fcb68e93c12f731b801b1
  
  
  
  Deriving all enabled types except hgchangesets and filenodes
  Success!


-- Create a commit in repo_b to update its submodule pointer from the large repo
-- EXPECT: commit is backsynced because the submodule expansion remains valid
  $ update_repo_b_submodule_pointer_in_large_repo
  
  
  NOTE: Create a commit in repo_b
  REPO_B_BONSAI: 9a7eca2ee4e942d3e2cbbe854063f7a7de9c41ae64585c9929479d2d4e185bef
  REPO_B_GIT_COMMIT_HASH: 63cddb76ed45b54c55223cba2ece9edd99ffed0e

  $ backsync_get_info_and_derive_data
  Processing commit: Valid repo_b submodule version bump from large repo
  Commit hash: 82ddf52981e5d88e0ba9c5c40a4119c0b3e79791
  Success!
  
  
  Submodule repo commit info using admin:
  BonsaiChangesetId: e1f796a6f06b7aa05fe731821fec48c456f11e9bf44aee5439d112e6c28a0513
  Message: Valid repo_b submodule version bump from large repo
  FileChanges:
  	 ADDED/MODIFIED: git-repo-b b4d69e88f92745803d6fa3a8ff2848098fba6d471db8f40c19f6536891fb5513
  
  
  
  Deriving all enabled types except hgchangesets and filenodes
  Success!


-- Update a recursive submodule pointer from the large repo
-- EXPECT: commit is backsynced because the submodule expansion remains valid
  $ update_repo_c_submodule_pointer_in_large_repo
  
  
  NOTE: Create a commit in repo_c and update its pointer in repo_b
  GIT_REPO_C_HEAD: 54f77f0efc9afcd04c3762a622f17ace83582151506f44d9eb0fcdd4b0e36bfa
  REPO_C_GIT_HASH: 5b447718cdc49d6289690f68b11f6b8a3002a396
  
  
  NOTE: Update repo_c submodule in git repo_b
  From $TESTTMP/git-repo-c
     810d4f5..5b44771  master_bookmark -> origin/master_bookmark
  Submodule path 'git-repo-c': checked out '5b447718cdc49d6289690f68b11f6b8a3002a396'
  GIT_REPO_B_HEAD: 065ee332e835cab7f781b87decb8c07bd5bb7fe129fc3aed47f4fa07697b65de
  REPO_B_GIT_COMMIT_HASH: 538924163436e89a3aa25a686075afb7182ec9c1
  Updating repo_b/repo_c submodule pointer to: 5b447718cdc49d6289690f68b11f6b8a3002a396
  Updating repo_b submodule pointer to: 538924163436e89a3aa25a686075afb7182ec9c1

  $ backsync_get_info_and_derive_data
  Processing commit: Valid repo_b and repo_c recursive submodule version bump from large repo
  Commit hash: 30c8d217612887e4fbdc67a3a99bfafbd30ff0c7
  Success!
  
  
  Submodule repo commit info using admin:
  BonsaiChangesetId: 478f652e09daebfe62efa122be1cb8a23495fb1da97e2d040750989c2bcdad08
  Message: Valid repo_b and repo_c recursive submodule version bump from large repo
  FileChanges:
  	 ADDED/MODIFIED: git-repo-b 49112bb3c7d5073a6fa052f26a20f029f2cdb847963c9120ddf073199fb3b5ab
  
  
  
  Deriving all enabled types except hgchangesets and filenodes
  Success!

-- -----------------------------------------------------------------------------
-- Test backsyncing changes that affect submodule expansions and are NOT VALID,
-- i.e. they break the submodule consistency of submodule expansions.
-- ALL SCENARIOS BELOW SHOULD FAIL TO BACKSYNC
-- -----------------------------------------------------------------------------


-- Change a small repo file inside a submodule expansion
-- First change the file without updating the submodule metadata file
  $ hg co -q .^ # go to previous commit because the current one doesn't sync
  $ echo "changing submodule expansion" > smallrepofolder1/git-repo-b/foo
  $ hg commit -Aq -m "Changing submodule expansion in large repo"
  $ backsync_get_info_and_derive_data
  Processing commit: Changing submodule expansion in large repo
  Commit hash: 17b64cd26d50139b93037e4aa7040cfaea104b15
  * Validation of submodule git-repo-b failed: Expansion of submodule git-repo-b changed without updating its metadata file smallrepofolder1/.x-repo-submodule-git-repo-b* (glob)
  [255]

-- Change a small repo file inside a recursive submodule expansion
  $ hg co -q .^ # go to previous commit because the current one doesn't sync
  $ echo "changing submodule expansion" > smallrepofolder1/git-repo-b/git-repo-c/choo
  $ hg commit -A -m "Changing recursive submodule expansion in large repo"
  $ backsync_get_info_and_derive_data
  Processing commit: Changing recursive submodule expansion in large repo
  Commit hash: 392ccb8b74534dfd35eb99e3f3f4ead1f0277e96
  * Validation of submodule expansion failed: * (glob)
  [255]

-- Delete submodule metadata file
  $ hg co -q .^ # go to previous commit because the current one doesn't sync
  $ rm smallrepofolder1/.x-repo-submodule-git-repo-b
  $ hg commit -q -A -m "Deleting repo_b submodule metadata file"
  $ backsync_get_info_and_derive_data
  Processing commit: Deleting repo_b submodule metadata file
  Commit hash: fc9ac6bc48350781bc9affc6125b3d3c234688d9
  * Submodule metadata file was deleted but 7 files in the submodule expansion were not* (glob)
  [255]


-- Delete recursive submodule metadata file
  $ hg co -q .^ # go to previous commit because the current one doesn't sync
  $ rm smallrepofolder1/git-repo-b/.x-repo-submodule-git-repo-c
  $ hg commit -q -A -m "Deleting repo_c recursive submodule metadata file"
  $ backsync_get_info_and_derive_data
  Processing commit: Deleting repo_c recursive submodule metadata file
  Commit hash: c5ba322776e498b34aee61308c3fb3590d09b0ce
  * Validation of submodule git-repo-b failed: Expansion of submodule git-repo-b changed without updating its metadata file smallrepofolder1/.x-repo-submodule-git-repo-b* (glob)
  [255]


-- Modify submodule metadata file
  $ hg co -q .^ # go to previous commit because the current one doesn't sync
  $ echo "change metadata file" > smallrepofolder1/.x-repo-submodule-git-repo-b
  $ hg commit -q -A -m "Change repo_b submodule metadata file"
  $ backsync_get_info_and_derive_data
  Processing commit: Change repo_b submodule metadata file
  Commit hash: cbf713928ec94ec9bf2d4eee8f9247d3001fc291
  * Validation of submodule git-repo-b failed: Failed to fetch content from content id 6a979ab567aa7b62632d9738f1e98bae548d7dd854ea57fe7bb1e25a19b7c78a file containing the submodule's git commit hash: Fetched content length (21) is not the expected size (40)* (glob)
  [255]


-- Modify recursive submodule metadata file
  $ hg co -q .^ # go to previous commit because the current one doesn't sync
  $ echo "change metadata file" > smallrepofolder1/git-repo-b/.x-repo-submodule-git-repo-c
  $ hg commit -q -A -m "Change repo_c recursive submodule metadata file"
  $ backsync_get_info_and_derive_data
  Processing commit: Change repo_c recursive submodule metadata file
  Commit hash: 02285a4ca81aa356b80d8dc2daf095822e0187d5
  * Validation of submodule expansion failed: * (glob)
  [255]



-- Delete submodule expansion
  $ hg co -q .^ # go to previous commit because the current one doesn't sync
  $ rm -rf smallrepofolder1/git-repo-b
  $ hg commit -q -A -m "Delete repo_b submodule expansion"
  $ backsync_get_info_and_derive_data
  Processing commit: Delete repo_b submodule expansion
  Commit hash: 6bb6f4620ca201142b5fd0d42486879a04158229
  * Validation of submodule git-repo-b failed: Expansion of submodule git-repo-b changed without updating its metadata file smallrepofolder1/.x-repo-submodule-git-repo-b* (glob)
  [255]

-- Delete recursive submodule expansion
  $ hg co -q .^ # go to previous commit because the current one doesn't sync
  $ rm -rf smallrepofolder1/git-repo-b/git-repo-c
  $ hg commit -q -A -m "Delete repo_c recursive submodule expansion"
  $ backsync_get_info_and_derive_data
  Processing commit: Delete repo_c recursive submodule expansion
  Commit hash: f229a7929d48e8d5e1e461443c41071b2c0be99e
  * Validation of submodule expansion failed: * (glob)
  [255]



  $ hg_log -r "sort(all(), desc)"
  @  f229a7929d48 Delete repo_c recursive submodule expansion
  │
  │ o  6bb6f4620ca2 Delete repo_b submodule expansion
  ├─╯
  │ o  02285a4ca81a Change repo_c recursive submodule metadata file
  ├─╯
  │ o  cbf713928ec9 Change repo_b submodule metadata file
  ├─╯
  │ o  c5ba322776e4 Deleting repo_c recursive submodule metadata file
  ├─╯
  │ o  fc9ac6bc4835 Deleting repo_b submodule metadata file
  ├─╯
  │ o  392ccb8b7453 Changing recursive submodule expansion in large repo
  ├─╯
  │ o  17b64cd26d50 Changing submodule expansion in large repo
  ├─╯
  │ o  30c8d2176128 Valid repo_b and repo_c recursive submodule version bump from large repo
  ├─╯
  o  82ddf52981e5 Valid repo_b submodule version bump from large repo
  │
  o  35e70dc7f37c Changing small repo in large repo (not submodule)
  │
  o  48021e7aeafd Changing large repo file
  │
  o  d246b01a5a5b Remove repo C submodule from repo A
  │
  o  d3dae76d4349 Update submodule B in repo A
  │
  o  ada44b220ff8 Change directly in A
  │
  o  e2b260a2b04f Added git repo C as submodule directly in A
  │
  o    c0240984981f [MEGAREPO GRADUAL MERGE] gradual merge (7)
  ├─╮
  │ o    eb7057489fd5 [MEGAREPO GRADUAL MERGE] gradual merge (6)
  │ ├─╮
  │ │ o    67ce5c45ccfe [MEGAREPO GRADUAL MERGE] gradual merge (5)
  │ │ ├─╮
  │ │ │ o    3d5d1c2ca884 [MEGAREPO GRADUAL MERGE] gradual merge (4)
  │ │ │ ├─╮
  │ │ │ │ o    19d222a5612c [MEGAREPO GRADUAL MERGE] gradual merge (3)
  │ │ │ │ ├─╮
  │ │ │ │ │ o    b019b0d36eb2 [MEGAREPO GRADUAL MERGE] gradual merge (2)
  │ │ │ │ │ ├─╮
  │ │ │ │ │ │ o    c40e45f2cc7c [MEGAREPO GRADUAL MERGE] gradual merge (1)
  │ │ │ │ │ │ ├─╮
  │ │ │ │ │ │ │ o    aea509f84730 [MEGAREPO GRADUAL MERGE] gradual merge (0)
  │ │ │ │ │ │ │ ├─╮
  │ │ │ │ │ │ │ │ o  10dab983a27f [MEGAREPO DELETE] deletion commits for merge into large repo (6)
  │ │ │ │ │ │ ├───╯
  │ │ │ │ │ │ o │  9f34257829fb [MEGAREPO DELETE] deletion commits for merge into large repo (5)
  │ │ │ │ │ ├─╯ │
  │ │ │ │ │ o   │  b3109b39500f [MEGAREPO DELETE] deletion commits for merge into large repo (4)
  │ │ │ │ ├─╯   │
  │ │ │ │ o     │  43f727449960 [MEGAREPO DELETE] deletion commits for merge into large repo (3)
  │ │ │ ├─╯     │
  │ │ │ o       │  9d59171d496f [MEGAREPO DELETE] deletion commits for merge into large repo (2)
  │ │ ├─╯       │
  │ │ o         │  5d6979a70f2b [MEGAREPO DELETE] deletion commits for merge into large repo (1)
  │ ├─╯         │
  │ o           │  c1f01db6a932 [MEGAREPO DELETE] deletion commits for merge into large repo (0)
  ├─╯           │
  o             │  1f9d3769f8c2 Added git repo B as submodule in A
  │             │
  o             │  e2c69ce8cc11 Add regular_dir/aardvar
  │             │
  o             │  df9086c77129 Add root_file
                │
                o  54a6db91baf1 L_A
  
