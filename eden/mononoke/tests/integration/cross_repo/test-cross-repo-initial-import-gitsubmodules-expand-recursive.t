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

-- Define the large and small repo ids and names before calling any helpers
  $ export LARGE_REPO_NAME="large_repo"
  $ export LARGE_REPO_ID=10
  $ export SUBMODULE_REPO_NAME="small_repo"
  $ export SUBMODULE_REPO_ID=11

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"
  $ . "${TEST_FIXTURES}/library-xrepo-git-submodule-expansion.sh"

Avoid local clone error "fatal: transport 'file' not allowed" in new Git versions (see CVE-2022-39253).
  $ export XDG_CONFIG_HOME=$TESTTMP
  $ git config --global protocol.file.allow always


Run the x-repo with submodules setup  
  $ ENABLE_API_WRITES=1 REPOID="$REPO_C_ID" REPONAME="repo_c" setup_common_config "$REPOTYPE"
  $ ENABLE_API_WRITES=1 REPOID="$REPO_B_ID" REPONAME="repo_b" setup_common_config "$REPOTYPE"
  $ run_common_xrepo_sync_with_gitsubmodules_setup
  $ set_git_submodules_action_in_config_version "$LATEST_CONFIG_VERSION_NAME" "$SUBMODULE_REPO_ID" 3 # 3=expand
  $ set_git_submodule_dependencies_in_config_version "$LATEST_CONFIG_VERSION_NAME" \
  > "$SUBMODULE_REPO_ID" "{\"git-repo-b\": $REPO_B_ID, \"git-repo-b/git-repo-c\": $REPO_C_ID, \"repo_c\": $REPO_C_ID}"


Create a commit in the large repo
  $ testtool_drawdag -R "$LARGE_REPO_NAME" --no-default-files <<EOF
  > L_A
  > # modify: L_A "file_in_large_repo.txt" "first file"
  > # bookmark: L_A master
  > EOF
  L_A=b006a2b1425af8612bc80ff4aa9fa8a1a2c44936ad167dd21cb9af2a9a0248c4

Setup git repos A, B and C
  $ setup_git_repos_a_b_c
  
  
  NOTE: Setting up git repo C to be used as submodule in git repo B
  114b61c Add hoo/qux
  7f760d8 Add choo
  
  
  NOTE: Setting up git repo B to be used as submodule in git repo A
  Cloning into '$TESTTMP/git-repo-b/git-repo-c'...
  done.
  776166f Added git repo C as submodule in B
  b7dc5d8 Add bar/zoo
  1c7ecd4 Add foo
  .
  |-- .gitmodules
  |-- bar
  |   `-- zoo
  |-- foo
  `-- git-repo-c
      |-- choo
      `-- hoo
          `-- qux
  
  3 directories, 5 files
  
  
  NOTE: Setting up git repo A
  Cloning into '$TESTTMP/git-repo-a/git-repo-b'...
  done.
  f3ce0ee Added git repo B as submodule in A
  ad7b606 Add regular_dir/aardvar
  8c33a27 Add root_file
  Cloning into '$TESTTMP/git-repo-a/repo_c'...
  done.
  .
  |-- .gitmodules
  |-- duplicates
  |   |-- x
  |   |-- y
  |   `-- z
  |-- git-repo-b
  |   |-- .gitmodules
  |   |-- bar
  |   |   `-- zoo
  |   |-- foo
  |   `-- git-repo-c
  |-- regular_dir
  |   `-- aardvar
  |-- repo_c
  |   |-- choo
  |   `-- hoo
  |       `-- qux
  `-- root_file
  
  7 directories, 11 files

Import all git repos into Mononoke
  $ gitimport_repos_a_b_c
  
  
  NOTE: Importing repos in reverse dependency order, C, B then A
  
  GIT_REPO_A_HEAD: eef414bd5fc8f7dcc129318276af6945117fe32bb5cfda6b0e6d43036107f61c
  
  GIT_REPO_A_HEAD_PARENT: c33eeb91423c021a4d9d57f2efbb08185c77d89b9141433c666b84240395f0c5

Merge repo A into the large repo
  $ merge_repo_a_to_large_repo
  
  
  NOTE: Importing repo A commits into large repo
  IMPORT_CONFIG_VERSION_NAME: INITIAL_IMPORT_SYNC_CONFIG
  FINAL_CONFIG_VERSION_NAME: INITIAL_IMPORT_SYNC_CONFIG
  Large repo MASTER_BOOKMARK_NAME: master
  
  GIT_REPO_A_HEAD: eef414bd5fc8f7dcc129318276af6945117fe32bb5cfda6b0e6d43036107f61c
  
  GIT_REPO_A_HEAD_PARENT: c33eeb91423c021a4d9d57f2efbb08185c77d89b9141433c666b84240395f0c5
  
  
  NOTE: Running initial import
  Starting session with id * (glob)
  Checking if * (glob)
  Syncing c33eeb91423c021a4d9d57f2efbb08185c77d89b9141433c666b84240395f0c5 for inital import
  Source repo: small_repo / Target repo: large_repo
  Found * unsynced ancestors (glob)
  changeset * synced as * in * (glob)
  successful sync of head * (glob)
  
  
  NOTE: Large repo bookmarks
  54a6db91baf1c10921369339b50e5a174a7ca82e master
  
  IMPORTED_HEAD: 6e3217760eada6926186d7cb48f4f24bd8a734ad615aec528065a0912dec6cba
  
  
  
  NOTE: Creating gradual merge commit
  using repo "large_repo" repoid RepositoryId(10)
  changeset resolved as: ChangesetId(Blake2(6e3217760eada6926186d7cb48f4f24bd8a734ad615aec528065a0912dec6cba))
  changeset resolved as: ChangesetId(Blake2(b43576e9e9685513cf91adbe5fb817cbe2837ba9a4dca12a4c64a6aebfe09780))
  Finding all commits to merge...
  2 total commits to merge
  Finding commits that haven't been merged yet...
  changeset resolved as: ChangesetId(Blake2(b006a2b1425af8612bc80ff4aa9fa8a1a2c44936ad167dd21cb9af2a9a0248c4))
  merging 1 commits
  Preparing to merge 6e3217760eada6926186d7cb48f4f24bd8a734ad615aec528065a0912dec6cba
  changeset resolved as: ChangesetId(Blake2(b006a2b1425af8612bc80ff4aa9fa8a1a2c44936ad167dd21cb9af2a9a0248c4))
  Created merge changeset 0e686785fb529cc8c6cba55b905da5a55bb53b32056a139b9912e50c3d36d26d
  Generated hg changeset 2b82a445059af7e1904a0b7a0d73a92b32e06f3e
  Now running pushrebase...
  Pushrebased to 0e686785fb529cc8c6cba55b905da5a55bb53b32056a139b9912e50c3d36d26d
  
  
  NOTE: Changing commit sync mapping version
  Starting session with id * (glob)
  changeset resolved as: ChangesetId(Blake2(eef414bd5fc8f7dcc129318276af6945117fe32bb5cfda6b0e6d43036107f61c))
  Checking if eef414bd5fc8f7dcc129318276af6945117fe32bb5cfda6b0e6d43036107f61c is already synced 11->10
  Changing mapping version during pushrebase to INITIAL_IMPORT_SYNC_CONFIG
  1 unsynced ancestors of eef414bd5fc8f7dcc129318276af6945117fe32bb5cfda6b0e6d43036107f61c
  UNSAFE: changing mapping version during pushrebase to INITIAL_IMPORT_SYNC_CONFIG
  syncing eef414bd5fc8f7dcc129318276af6945117fe32bb5cfda6b0e6d43036107f61c via pushrebase for master
  changeset eef414bd5fc8f7dcc129318276af6945117fe32bb5cfda6b0e6d43036107f61c synced as aab4b2b2a2fa16ef36968a6f3c98f89f27726b30f817ca7211b84055c4e3fed4 in * (glob)
  successful sync
  
  SYNCED_HEAD: aab4b2b2a2fa16ef36968a6f3c98f89f27726b30f817ca7211b84055c4e3fed4
  
  @  9a84b278c0f9 Added git repo C as submodule directly in A
  │   smallrepofolder1/.gitmodules              |  3 +++
  │   smallrepofolder1/.x-repo-submodule-repo_c |  1 +
  │   smallrepofolder1/repo_c/choo              |  1 +
  │   smallrepofolder1/repo_c/hoo/qux           |  1 +
  │   4 files changed, 6 insertions(+), 0 deletions(-)
  │
  o    2b82a445059a [MEGAREPO GRADUAL MERGE] gradual merge (0)
  ├─╮   smallrepofolder1/.gitmodules                             |  3 +++
  │ │   smallrepofolder1/.x-repo-submodule-git-repo-b            |  1 +
  │ │   smallrepofolder1/duplicates/x                            |  1 +
  │ │   smallrepofolder1/duplicates/y                            |  1 +
  │ │   smallrepofolder1/duplicates/z                            |  1 +
  │ │   smallrepofolder1/git-repo-b/.gitmodules                  |  3 +++
  │ │   smallrepofolder1/git-repo-b/.x-repo-submodule-git-repo-c |  1 +
  │ │   smallrepofolder1/git-repo-b/bar/zoo                      |  1 +
  │ │   smallrepofolder1/git-repo-b/foo                          |  1 +
  │ │   smallrepofolder1/git-repo-b/git-repo-c/choo              |  1 +
  │ │   smallrepofolder1/git-repo-b/git-repo-c/hoo/qux           |  1 +
  │ │   smallrepofolder1/regular_dir/aardvar                     |  1 +
  │ │   smallrepofolder1/root_file                               |  1 +
  │ │   13 files changed, 17 insertions(+), 0 deletions(-)
  │ │
  │ o  1f9d3769f8c2 Added git repo B as submodule in A
  │ │   smallrepofolder1/.gitmodules                             |  3 +++
  │ │   smallrepofolder1/.x-repo-submodule-git-repo-b            |  1 +
  │ │   smallrepofolder1/git-repo-b/.gitmodules                  |  3 +++
  │ │   smallrepofolder1/git-repo-b/.x-repo-submodule-git-repo-c |  1 +
  │ │   smallrepofolder1/git-repo-b/bar/zoo                      |  1 +
  │ │   smallrepofolder1/git-repo-b/foo                          |  1 +
  │ │   smallrepofolder1/git-repo-b/git-repo-c/choo              |  1 +
  │ │   smallrepofolder1/git-repo-b/git-repo-c/hoo/qux           |  1 +
  │ │   8 files changed, 12 insertions(+), 0 deletions(-)
  │ │
  │ o  e2c69ce8cc11 Add regular_dir/aardvar
  │ │   smallrepofolder1/regular_dir/aardvar |  1 +
  │ │   1 files changed, 1 insertions(+), 0 deletions(-)
  │ │
  │ o  df9086c77129 Add root_file
  │     smallrepofolder1/duplicates/x |  1 +
  │     smallrepofolder1/duplicates/y |  1 +
  │     smallrepofolder1/duplicates/z |  1 +
  │     smallrepofolder1/root_file    |  1 +
  │     4 files changed, 4 insertions(+), 0 deletions(-)
  │
  o  54a6db91baf1 L_A
      file_in_large_repo.txt |  1 +
      1 files changed, 1 insertions(+), 0 deletions(-)
  
  
  
  Running mononoke_admin to verify mapping
  
  RewrittenAs([(ChangesetId(Blake2(eef414bd5fc8f7dcc129318276af6945117fe32bb5cfda6b0e6d43036107f61c)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
  
  Deriving all the enabled derived data types
  Large repo tree:
  .
  |-- file_in_large_repo.txt
  `-- smallrepofolder1
      |-- .gitmodules
      |-- .x-repo-submodule-git-repo-b
      |-- .x-repo-submodule-repo_c
      |-- duplicates
      |   |-- x
      |   |-- y
      |   `-- z
      |-- git-repo-b
      |   |-- .gitmodules
      |   |-- .x-repo-submodule-git-repo-c
      |   |-- bar
      |   |   `-- zoo
      |   |-- foo
      |   `-- git-repo-c
      |       |-- choo
      |       `-- hoo
      |           `-- qux
      |-- regular_dir
      |   `-- aardvar
      |-- repo_c
      |   |-- choo
      |   `-- hoo
      |       `-- qux
      `-- root_file
  
  9 directories, 17 files
  
  
  NOTE: Count underived data types
  aab4b2b2a2fa16ef36968a6f3c98f89f27726b30f817ca7211b84055c4e3fed4: 0
  aab4b2b2a2fa16ef36968a6f3c98f89f27726b30f817ca7211b84055c4e3fed4: 0
  aab4b2b2a2fa16ef36968a6f3c98f89f27726b30f817ca7211b84055c4e3fed4: 0

Make changes to submodule and make sure they're synced properly
  $ make_changes_to_git_repos_a_b_c
  
  
  NOTE: Make changes to repo C
  810d4f5 commit #4 in repo C
  55e8308 commit #3 in repo C
  114b61c Add hoo/qux
  7f760d8 Add choo
  
  
  NOTE: Update those changes in repo B
  From $TESTTMP/git-repo-c
     114b61c..810d4f5  master     -> origin/master
  Submodule path 'git-repo-c': checked out '810d4f53650b0fd891ad367ccfd8fa6067d93937'
  0597690 Delete files in repo B
  c9e2185 Update submodule C in repo B
  776166f Added git repo C as submodule in B
  b7dc5d8 Add bar/zoo
  1c7ecd4 Add foo
  
  
  NOTE: Update those changes in repo A
  
  
  NOTE: Update submodule b in A
  From $TESTTMP/git-repo-b
     776166f..0597690  master     -> origin/master
  Submodule path 'git-repo-b': checked out '0597690a839ce11a250139dae33ee85d9772a47a'
  From $TESTTMP/git-repo-c
     114b61c..810d4f5  master     -> origin/master
  Submodule path 'repo_c': checked out '810d4f53650b0fd891ad367ccfd8fa6067d93937'
  
  
  NOTE: Then delete repo C submodule used directly in repo A
  Cleared directory 'repo_c'
  Submodule 'repo_c' (../git-repo-c) unregistered for path 'repo_c'
  rm 'repo_c'
  6775096 Remove repo C submodule from repo A
  5f6b001 Update submodule B in repo A
  de77178 Change directly in A
  3a41dad Added git repo C as submodule directly in A
  f3ce0ee Added git repo B as submodule in A
  ad7b606 Add regular_dir/aardvar
  8c33a27 Add root_file

  $ mononoke_newadmin bookmarks -R "$SUBMODULE_REPO_NAME" list -S hg
  heads/master

Import the changes from the git repos B and C into their Mononoke repos
  $ REPOID="$REPO_C_ID" QUIET_LOGGING_LOG_FILE="$TESTTMP/gitimport_repo_c.out"  \
  > quiet gitimport "$GIT_REPO_C" --bypass-derived-data-backfilling \
  > --bypass-readonly --generate-bookmarks missing-for-commit "$GIT_REPO_C_HEAD"

  $ REPOID="$REPO_B_ID" QUIET_LOGGING_LOG_FILE="$TESTTMP/gitimport_repo_b.out" \
  > quiet gitimport "$GIT_REPO_B" --bypass-derived-data-backfilling \
  > --bypass-readonly --generate-bookmarks missing-for-commit "$GIT_REPO_B_HEAD"

Set up live forward syncer, which should sync all commits in small repo's (repo A)
heads/master bookmark to large repo's master bookmark via pushrebase
  $ touch $TESTTMP/xreposync.out
  $ with_stripped_logs mononoke_x_repo_sync_forever "$SUBMODULE_REPO_ID" "$LARGE_REPO_ID" 

Import the changes from git repo A into its Mononoke repo. They should be automatically
forward synced to the large repo
  $ REPOID="$SUBMODULE_REPO_ID" with_stripped_logs gitimport "$GIT_REPO_A" --bypass-derived-data-backfilling \
  > --bypass-readonly --generate-bookmarks missing-for-commit "$GIT_REPO_A_HEAD" > $TESTTMP/gitimport_output

  $ QUIET_LOGGING_LOG_FILE="$TESTTMP/xrepo_sync_last_logs.out" with_stripped_logs wait_for_xrepo_sync 2

  $ cd "$TESTTMP/$LARGE_REPO_NAME"
  $ hg pull -q 
  $ hg co -q master

  $ hg log --graph -T '{node} {desc}\n' -r "all()"
  @  c10f082fbef14d4eb82073a1578effe603e36ee7 Remove repo C submodule from repo A
  │
  o  46557285148375de0b93927ba58cde48a120c4d8 Update submodule B in repo A
  │
  o  27380bc5553eb680703c330fc83a614c6c10034c Change directly in A
  │
  o  9a84b278c0f94d79a1a2b700ae51ebc9c122cd0b Added git repo C as submodule directly in A
  │
  o    2b82a445059af7e1904a0b7a0d73a92b32e06f3e [MEGAREPO GRADUAL MERGE] gradual merge (0)
  ├─╮
  │ o  1f9d3769f8c22b50db3ed0105c9d0e9490bbe7e9 Added git repo B as submodule in A
  │ │
  │ o  e2c69ce8cc11691984e50e6023f4bbf4271aa4c3 Add regular_dir/aardvar
  │ │
  │ o  df9086c771290c305c738040313bf1cc5759eba9 Add root_file
  │
  o  54a6db91baf1c10921369339b50e5a174a7ca82e L_A
  

Check that deletions were made properly, i.e. submodule in repo_c was entirely
deleted and the files deleted in repo B were deleted inside its copy.
  $ hg show --stat -T 'commit: {node}\n{desc}\n' .
  commit: c10f082fbef14d4eb82073a1578effe603e36ee7
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

Check that the diff that updates the submodule generates the correct delta
(i.e. instead of copying the entire working copy of the submodule every time)
  $ hg show --stat -T 'commit: {node}\n{desc}\n' .^
  commit: 46557285148375de0b93927ba58cde48a120c4d8
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

Also check that our two binaries that can verify working copy are able to deal with expansions
  $ REPOIDLARGE=$LARGE_REPO_ID REPOIDSMALL=$SUBMODULE_REPO_ID verify_wc master |& strip_glog

The check-push-redirection-prereqs should behave the same both ways but let's verify it (we had bugs where it didn't)
(those outputs are still not correct but that's expected)
  $ quiet_grep "all is well" -- with_stripped_logs megarepo_tool_multirepo --source-repo-id $SUBMODULE_REPO_ID --target-repo-id $LARGE_REPO_ID check-push-redirection-prereqs "heads/master" "master" "$LATEST_CONFIG_VERSION_NAME" | strip_glog | tee $TESTTMP/push_redir_prereqs_small_large
  all is well!
  $ quiet_grep "all is well" -- megarepo_tool_multirepo --source-repo-id $LARGE_REPO_ID --target-repo-id $SUBMODULE_REPO_ID check-push-redirection-prereqs "master" "heads/master" "$LATEST_CONFIG_VERSION_NAME" | strip_glog | tee $TESTTMP/push_redir_prereqs_large_small
  all is well!
  $ diff -wbBdu $TESTTMP/push_redir_prereqs_small_large $TESTTMP/push_redir_prereqs_large_small

Let's corrupt the expansion and check if validation complains
(those outputs are still not correct but that's expected)
  $ echo corrupt > smallrepofolder1/git-repo-b/git-repo-c/choo3 
  $ echo corrupt > smallrepofolder1/.x-repo-submodule-git-repo-b
  $ hg commit -m "submodule corruption"
  $ hg push -q --to master
  $ quiet_grep "mismatch" -- with_stripped_logs megarepo_tool_multirepo --source-repo-id $SUBMODULE_REPO_ID --target-repo-id $LARGE_REPO_ID check-push-redirection-prereqs "heads/master" "master" "$LATEST_CONFIG_VERSION_NAME" | strip_glog | tee $TESTTMP/push_redir_prereqs_small_large
  submodule expansion mismatch: Failed to fetch content from content id 06a434694d9172d617062abd92f015f73978fb17dd6bcc54e708cd2c6f247970 file containing the submodule's git commit hash

  $ quiet_grep "mismatch" -- megarepo_tool_multirepo --source-repo-id $LARGE_REPO_ID --target-repo-id $SUBMODULE_REPO_ID check-push-redirection-prereqs "master" "heads/master" "$LATEST_CONFIG_VERSION_NAME" | sort | tee $TESTTMP/push_redir_prereqs_large_small
  submodule expansion mismatch: Failed to fetch content from content id 06a434694d9172d617062abd92f015f73978fb17dd6bcc54e708cd2c6f247970 file containing the submodule's git commit hash

  $ diff -wbBdu $TESTTMP/push_redir_prereqs_small_large $TESTTMP/push_redir_prereqs_large_small

-- ------------------------------------------------------------------------------
-- Test hgedenapi xrepo lookup with commits that are synced

-- Helper function to look for the mapping in the database using admin and then
-- call hgedenpi committranslateids endpoint from large to small.
  $ function check_mapping_and_run_xrepo_lookup_large_to_small {
  >   local hg_hash=$1; shift;
  >   
  >   printf "Check mapping in database with Mononoke admin\n"
  >   with_stripped_logs mononoke_admin_source_target $LARGE_REPO_ID $SUBMODULE_REPO_ID \
  >     crossrepo map $hg_hash | rg -v "using repo"
  >   
  >   printf "\n\nCall hgedenapi committranslateids\n" 
  >   
  >   REPONAME=$LARGE_REPO_NAME hgedenapi debugapi -e committranslateids \
  >     -i "[{'Hg': '$hg_hash'}]" -i "'Bonsai'" -i None -i "'$SUBMODULE_REPO_NAME'"
  >   
  > }


-- Looking up synced commits from large to small.
-- EXPECT: all of them should return the same value as mapping check using admin

-- Commit: Change directly in A
  $ check_mapping_and_run_xrepo_lookup_large_to_small 27380bc5553eb680703c330fc83a614c6c10034c
  Check mapping in database with Mononoke admin
  changeset resolved as: ChangesetId(Blake2(6c74b3e9f4ac9ae9ded1cc12d02fa23912926e30f60bc7c45595ad2db88c50af))
  RewrittenAs([(ChangesetId(Blake2(4aee0499ea629ebcd9d0e4be89267d7a4eab5e72f988c20a392d59081db0c32a)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
  
  
  Call hgedenapi committranslateids
  [{"commit": {"Hg": bin("27380bc5553eb680703c330fc83a614c6c10034c")},
    "translated": {"Bonsai": bin("4aee0499ea629ebcd9d0e4be89267d7a4eab5e72f988c20a392d59081db0c32a")}}]

-- Commit: Update submodule B in repo A
  $ check_mapping_and_run_xrepo_lookup_large_to_small 46557285148375de0b93927ba58cde48a120c4d8
  Check mapping in database with Mononoke admin
  changeset resolved as: ChangesetId(Blake2(6b141d0416df9f0e3054b38047779bb899972180d86bab364216c5f6ab04d402))
  RewrittenAs([(ChangesetId(Blake2(b86f7426fc1fe95e22b6bef591e7ba9c8385b86f7b85abd3a377f941d39522af)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
  
  
  Call hgedenapi committranslateids
  [{"commit": {"Hg": bin("46557285148375de0b93927ba58cde48a120c4d8")},
    "translated": {"Bonsai": bin("b86f7426fc1fe95e22b6bef591e7ba9c8385b86f7b85abd3a377f941d39522af")}}]

-- Check an original commit from small repo (before merge)
-- Commit: Add regular_dir/aardvar
  $ check_mapping_and_run_xrepo_lookup_large_to_small e2c69ce8cc11691984e50e6023f4bbf4271aa4c3
  Check mapping in database with Mononoke admin
  changeset resolved as: ChangesetId(Blake2(b43576e9e9685513cf91adbe5fb817cbe2837ba9a4dca12a4c64a6aebfe09780))
  RewrittenAs([(ChangesetId(Blake2(856b09638e2550d912282c5a9e8bd47fdf1a899545f9f4a05430a8dc7be1f768)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
  
  
  Call hgedenapi committranslateids
  [{"commit": {"Hg": bin("e2c69ce8cc11691984e50e6023f4bbf4271aa4c3")},
    "translated": {"Bonsai": bin("856b09638e2550d912282c5a9e8bd47fdf1a899545f9f4a05430a8dc7be1f768")}}]



-- ------------------------------------------------------------------------------
-- Test backsyncing (i.e. large to small)

  $ cd "$TESTTMP/$LARGE_REPO_NAME" || exit
  $ hg pull -q && hg co -q master  
  $ hgmn status
  $ hgmn co -q .^ # go before the commit that corrupts submodules
  $ hgmn status
  $ enable commitcloud infinitepush # to push commits to server
  $ function hg_log() { 
  >   hgmn log --graph -T '{node|short} {desc}\n' "$@" 
  > }

  $ hg_log
  o  7ac324cc4ce2 submodule corruption
  │
  @  c10f082fbef1 Remove repo C submodule from repo A
  │
  o  465572851483 Update submodule B in repo A
  │
  o  27380bc5553e Change directly in A
  │
  o  9a84b278c0f9 Added git repo C as submodule directly in A
  │
  o    2b82a445059a [MEGAREPO GRADUAL MERGE] gradual merge (0)
  ├─╮
  │ o  1f9d3769f8c2 Added git repo B as submodule in A
  │ │
  │ o  e2c69ce8cc11 Add regular_dir/aardvar
  │ │
  │ o  df9086c77129 Add root_file
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
  >   REPONAME="$LARGE_REPO_NAME" hgedenapi cloud backup -q
  >   COMMIT_TO_SYNC=$(hgmn whereami)
  >   COMMIT_TITLE=$(hgmn log -l1  -T "{truncate(desc, 1)}")
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
  >   printf "\n\nSubmodule repo commit info using newadmin:\n"
  >   mononoke_newadmin fetch -R "$SUBMODULE_REPO_NAME" -i "$SYNCED_BONSAI" \
  >     | rg -v "Author"
  > 
  >   printf "\n\nDeriving all enabled types except hgchangesets and filenodes\n";
  >   (mononoke_newadmin derived-data -R "$SUBMODULE_REPO_NAME" derive -i $SYNCED_BONSAI \
  >     -T fsnodes -T unodes -T fastlog -T fsnodes -T blame -T changeset_info \
  >     -T skeleton_manifests -T deleted_manifest -T bssm_v3 \
  >     -T git_commits -T git_trees -T git_delta_manifests \
  >       && echo "Success!") 2>&1 | rg "Error|Success" || true;
  > }

-- Change a large repo file and try to backsync it to small repo
-- EXPECT: commit isn't synced and returns working copy equivalent instead
  $ echo "changing large repo file" > file_in_large_repo.txt
  $ hgmn commit -A -m "Changing large repo file" 
  $ backsync_get_info_and_derive_data
  Processing commit: Changing large repo file
  Commit hash: 7dc2c0583a5055929fd772d94a6db6b9a3680ff8
  Success!
  
  
  Submodule repo commit info using newadmin:
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
  $ hgmn commit -A -m "Changing small repo in large repo (not submodule)" 
  $ backsync_get_info_and_derive_data
  Processing commit: Changing small repo in large repo (not submodule)
  Commit hash: fac42b07b3b39cfb46f5eba3eb736126231ba3c6
  Success!
  
  
  Submodule repo commit info using newadmin:
  BonsaiChangesetId: ee442222a80354fc6e4b8dc910d9938b73a9780608f1762ccd9836dbf2319422
  Message: Changing small repo in large repo (not submodule)
  FileChanges:
  	 ADDED/MODIFIED: regular_dir/aardvar 58186314bed8b207f5f63a4a58aa858e715f25225a6fcb68e93c12f731b801b1
  
  
  
  Deriving all enabled types except hgchangesets and filenodes
  Success!

-- -----------------------------------------------------------------------------
-- Test backsyncing changes that affect submodule expansions, which is 
-- not supported yet.
-- ALL SCENARIOS BELOW SHOULD FAIL TO BACKSYNC
-- -----------------------------------------------------------------------------

TODO(T179530927): properly support backsyncing with submodule expansion

-- Change a small repo file inside a submodule expansion
  $ echo "changing submodule expansion" > smallrepofolder1/git-repo-b/foo
  $ hgmn commit -A -m "Changing submodule expansion in large repo" 
  adding smallrepofolder1/git-repo-b/foo
  $ backsync_get_info_and_derive_data
  Processing commit: Changing submodule expansion in large repo
  Commit hash: 229fe03c0d3780e5de559833a6ba5d0d32b99451
  *error: Changeset can't be synced from large to small repo because it modifies the expansion of submodules* (glob)
  [255]
 

-- Change a small repo file inside a recursive submodule expansion
  $ hgmn co -q .^ # go to previous commit because the current one doesn't sync
  $ echo "changing submodule expansion" > smallrepofolder1/git-repo-b/git-repo-c/choo
  $ hgmn commit -A -m "Changing recursive submodule expansion in large repo" 
  $ backsync_get_info_and_derive_data
  Processing commit: Changing recursive submodule expansion in large repo
  Commit hash: d06cd21a6e1a520d6e9cc193cc9ab92b06afc199
  *error: Changeset can't be synced from large to small repo because it modifies the expansion of submodules* (glob)
  [255]

-- Delete submodule metadata file
  $ hgmn co -q .^ # go to previous commit because the current one doesn't sync
  $ rm smallrepofolder1/.x-repo-submodule-git-repo-b
  $ hgmn commit -q -A -m "Deleting repo_b submodule metadata file" 
  $ backsync_get_info_and_derive_data
  Processing commit: Deleting repo_b submodule metadata file
  Commit hash: b25d2d2919aa11229071ca224d2c4e837689b46b
  *error: Changeset can't be synced from large to small repo because it modifies the expansion of submodules* (glob)
  [255]


-- Delete recursive submodule metadata file
  $ hgmn co -q .^ # go to previous commit because the current one doesn't sync
  $ rm smallrepofolder1/git-repo-b/.x-repo-submodule-git-repo-c
  $ hgmn commit -q -A -m "Deleting repo_c recursive submodule metadata file" 
  $ backsync_get_info_and_derive_data
  Processing commit: Deleting repo_c recursive submodule metadata file
  Commit hash: 1b19032a7f306819fc67546816bbea178842e152
  *error: Changeset can't be synced from large to small repo because it modifies the expansion of submodules* (glob)
  [255]


-- Modify submodule metadata file
  $ hgmn co -q .^ # go to previous commit because the current one doesn't sync
  $ echo "change metadata file" > smallrepofolder1/.x-repo-submodule-git-repo-b
  $ hgmn commit -q -A -m "Change repo_b submodule metadata file" 
  $ backsync_get_info_and_derive_data
  Processing commit: Change repo_b submodule metadata file
  Commit hash: 7a6d0f4b7ea2434a68dc45f56b56f4d26abf293d
  *error: Changeset can't be synced from large to small repo because it modifies the expansion of submodules* (glob)
  [255]


-- Modify recursive submodule metadata file
  $ hgmn co -q .^ # go to previous commit because the current one doesn't sync
  $ echo "change metadata file" > smallrepofolder1/git-repo-b/.x-repo-submodule-git-repo-c
  $ hgmn commit -q -A -m "Change repo_c recursive submodule metadata file" 
  $ backsync_get_info_and_derive_data
  Processing commit: Change repo_c recursive submodule metadata file
  Commit hash: 4740fc3bcff4482a13dbb8bf161a7e9f5b75d188
  *error: Changeset can't be synced from large to small repo because it modifies the expansion of submodules* (glob)
  [255]



-- Delete submodule expansion
  $ hgmn co -q .^ # go to previous commit because the current one doesn't sync
  $ rm -rf smallrepofolder1/git-repo-b
  $ hgmn commit -q -A -m "Delete repo_b submodule expansion" 
  $ backsync_get_info_and_derive_data
  Processing commit: Delete repo_b submodule expansion
  Commit hash: 0c5b59e1b3503d556f04f7fe5b9f2b5edef25a91
  *error: Changeset can't be synced from large to small repo because it modifies the expansion of submodules* (glob)
  [255]

-- Delete recursive submodule expansion
  $ hgmn co -q .^ # go to previous commit because the current one doesn't sync
  $ rm -rf smallrepofolder1/git-repo-b/git-repo-c
  $ hgmn commit -q -A -m "Delete repo_c recursive submodule expansion" 
  $ backsync_get_info_and_derive_data
  Processing commit: Delete repo_c recursive submodule expansion
  Commit hash: ea09438156c0b7290bcc655c6b11e3de39956241
  *error: Changeset can't be synced from large to small repo because it modifies the expansion of submodules* (glob)
  [255]



  $ hg_log -r "sort(all(), desc)"
  @  ea09438156c0 Delete repo_c recursive submodule expansion
  │
  │ o  0c5b59e1b350 Delete repo_b submodule expansion
  ├─╯
  │ o  4740fc3bcff4 Change repo_c recursive submodule metadata file
  ├─╯
  │ o  7a6d0f4b7ea2 Change repo_b submodule metadata file
  ├─╯
  │ o  1b19032a7f30 Deleting repo_c recursive submodule metadata file
  ├─╯
  │ o  b25d2d2919aa Deleting repo_b submodule metadata file
  ├─╯
  │ o  d06cd21a6e1a Changing recursive submodule expansion in large repo
  ├─╯
  │ o  229fe03c0d37 Changing submodule expansion in large repo
  ├─╯
  o  fac42b07b3b3 Changing small repo in large repo (not submodule)
  │
  o  7dc2c0583a50 Changing large repo file
  │
  │ o  7ac324cc4ce2 submodule corruption
  ├─╯
  o  c10f082fbef1 Remove repo C submodule from repo A
  │
  o  465572851483 Update submodule B in repo A
  │
  o  27380bc5553e Change directly in A
  │
  o  9a84b278c0f9 Added git repo C as submodule directly in A
  │
  o    2b82a445059a [MEGAREPO GRADUAL MERGE] gradual merge (0)
  ├─╮
  │ o  1f9d3769f8c2 Added git repo B as submodule in A
  │ │
  │ o  e2c69ce8cc11 Add regular_dir/aardvar
  │ │
  │ o  df9086c77129 Add root_file
  │
  o  54a6db91baf1 L_A
  
