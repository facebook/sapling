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

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"
  $ . "${TEST_FIXTURES}/library-xrepo-git-submodule-expansion.sh"

Avoid local clone error "fatal: transport 'file' not allowed" in new Git versions (see CVE-2022-39253).
  $ export XDG_CONFIG_HOME=$TESTTMP
  $ git config --global protocol.file.allow always


Run the x-repo with submodules setup  
  $ run_common_xrepo_sync_with_gitsubmodules_setup
  $ set_git_submodules_action_in_config_version "$LATEST_CONFIG_VERSION_NAME" "$SMALL_REPO_ID" 3
  $ set_git_submodule_dependencies_in_config_version "$LATEST_CONFIG_VERSION_NAME" \
  > "$SMALL_REPO_ID" '{"git-repo-b": 3, "git-repo-b/git-repo-c": 2, "repo_c": 2}'
  $ ENABLE_API_WRITES=1 REPOID="$REPO_C_ID" REPONAME="repo_c" setup_common_config "$REPOTYPE"
  $ ENABLE_API_WRITES=1 REPOID="$REPO_B_ID" REPONAME="repo_b" setup_common_config "$REPOTYPE"


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

Merge repo A into the large repo
  $ merge_repo_a_to_large_repo
  
  
  NOTE: Importing repo A commits into large repo
  Starting session with id * (glob)
  Checking if * (glob)
  syncing * (glob)
  Found * unsynced ancestors (glob)
  changeset * synced as * in * (glob)
  successful sync of head * (glob)
  
  
  NOTE: Large repo bookmarks
  54a6db91baf1c10921369339b50e5a174a7ca82e master
  
  
  NOTE: Creating gradual merge commit
  using repo "large_repo" repoid RepositoryId(0)
  changeset resolved as: ChangesetId(Blake2(bdf369cd5851a09ea0ff89c4df2f61254de3b2f33197a0e56f0ccdbcc2773655))
  changeset resolved as: ChangesetId(Blake2(e7ba6c7472c561619cf7fbe392ab881eec075f443e671a45c2f6a5f4effa9865))
  Finding all commits to merge...
  2 total commits to merge
  Finding commits that haven't been merged yet...
  changeset resolved as: ChangesetId(Blake2(b006a2b1425af8612bc80ff4aa9fa8a1a2c44936ad167dd21cb9af2a9a0248c4))
  merging 1 commits
  Preparing to merge bdf369cd5851a09ea0ff89c4df2f61254de3b2f33197a0e56f0ccdbcc2773655
  changeset resolved as: ChangesetId(Blake2(b006a2b1425af8612bc80ff4aa9fa8a1a2c44936ad167dd21cb9af2a9a0248c4))
  Created merge changeset c8c3a69055051bf32202e36013cdebbdeb75ca20d798cd50328fb12f8e29e150
  Generated hg changeset eae90b3bbae442f600e8903e5e3ce648e8c8c59e
  Now running pushrebase...
  Pushrebased to c8c3a69055051bf32202e36013cdebbdeb75ca20d798cd50328fb12f8e29e150
  
  SYNCHED_HEAD: bdf369cd5851a09ea0ff89c4df2f61254de3b2f33197a0e56f0ccdbcc2773655
  
  @    eae90b3bbae4 [MEGAREPO GRADUAL MERGE] gradual merge (0)
  ├─╮   smallrepofolder1/.gitmodules                   |  6 ++++++
  │ │   smallrepofolder1/.x-repo-submodule-git-repo-b  |  1 +
  │ │   smallrepofolder1/.x-repo-submodule-repo_c      |  1 +
  │ │   smallrepofolder1/duplicates/x                  |  1 +
  │ │   smallrepofolder1/duplicates/y                  |  1 +
  │ │   smallrepofolder1/duplicates/z                  |  1 +
  │ │   smallrepofolder1/git-repo-b/.gitmodules        |  3 +++
  │ │   smallrepofolder1/git-repo-b/bar/zoo            |  1 +
  │ │   smallrepofolder1/git-repo-b/foo                |  1 +
  │ │   smallrepofolder1/git-repo-b/git-repo-c/choo    |  1 +
  │ │   smallrepofolder1/git-repo-b/git-repo-c/hoo/qux |  1 +
  │ │   smallrepofolder1/regular_dir/aardvar           |  1 +
  │ │   smallrepofolder1/repo_c/choo                   |  1 +
  │ │   smallrepofolder1/repo_c/hoo/qux                |  1 +
  │ │   smallrepofolder1/root_file                     |  1 +
  │ │   15 files changed, 22 insertions(+), 0 deletions(-)
  │ │
  │ o  c1e1c514c4c2 Added git repo C as submodule directly in A
  │ │   smallrepofolder1/.gitmodules              |  3 +++
  │ │   smallrepofolder1/.x-repo-submodule-repo_c |  1 +
  │ │   smallrepofolder1/repo_c/choo              |  1 +
  │ │   smallrepofolder1/repo_c/hoo/qux           |  1 +
  │ │   4 files changed, 6 insertions(+), 0 deletions(-)
  │ │
  │ o  72a5410557a9 Added git repo B as submodule in A
  │ │   smallrepofolder1/.gitmodules                   |  3 +++
  │ │   smallrepofolder1/.x-repo-submodule-git-repo-b  |  1 +
  │ │   smallrepofolder1/git-repo-b/.gitmodules        |  3 +++
  │ │   smallrepofolder1/git-repo-b/bar/zoo            |  1 +
  │ │   smallrepofolder1/git-repo-b/foo                |  1 +
  │ │   smallrepofolder1/git-repo-b/git-repo-c/choo    |  1 +
  │ │   smallrepofolder1/git-repo-b/git-repo-c/hoo/qux |  1 +
  │ │   7 files changed, 11 insertions(+), 0 deletions(-)
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
  
  9 directories, 16 files

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

  $ mononoke_newadmin bookmarks -R "$SMALL_REPO_NAME" list -S hg
  heads/master

Import the changes from the git repos B and C into their Mononoke repos
  $ REPOID="$REPO_C_ID" quiet gitimport "$GIT_REPO_C" --bypass-derived-data-backfilling \
  > --bypass-readonly --generate-bookmarks missing-for-commit "$GIT_REPO_C_HEAD"

  $ REPOID="$REPO_B_ID" quiet gitimport "$GIT_REPO_B" --bypass-derived-data-backfilling \
  > --bypass-readonly --generate-bookmarks missing-for-commit "$GIT_REPO_B_HEAD"

Set up live forward syncer, which should sync all commits in small repo's (repo A)
heads/master bookmark to large repo's master bookmark via pushrebase
  $ touch $TESTTMP/xreposync.out
  $ with_stripped_logs mononoke_x_repo_sync_forever "$SMALL_REPO_ID" "$LARGE_REPO_ID" 

Import the changes from git repo A into its Mononoke repo. They should be automatically
forward synced to the large repo
  $ REPOID="$SMALL_REPO_ID" with_stripped_logs gitimport "$GIT_REPO_A" --bypass-derived-data-backfilling \
  > --bypass-readonly --generate-bookmarks missing-for-commit "$GIT_REPO_A_HEAD" > $TESTTMP/gitimport_output

  $ wait_for_xrepo_sync 2

  $ cd "$TESTTMP/$LARGE_REPO_NAME"
  $ hg pull -q 
  $ hg co -q master

  $ hg log --graph -T '{node|short} {desc}\n' -r "all()"
  @  6983801de625 Remove repo C submodule from repo A
  │
  o  ebb491080818 Update submodule B in repo A
  │
  o  8dac396dd939 Change directly in A
  │
  o    eae90b3bbae4 [MEGAREPO GRADUAL MERGE] gradual merge (0)
  ├─╮
  │ o  c1e1c514c4c2 Added git repo C as submodule directly in A
  │ │
  │ o  72a5410557a9 Added git repo B as submodule in A
  │ │
  │ o  e2c69ce8cc11 Add regular_dir/aardvar
  │ │
  │ o  df9086c77129 Add root_file
  │
  o  54a6db91baf1 L_A
  

Check that deletions were made properly, i.e. submodule in repo_c was entirely
deleted and the files deleted in repo B were deleted inside its copy.
  $ hg show --stat -T 'commit: {node}\n{desc}\n' .
  commit: 6983801de6259a0d33ce2984ba828ebdac2dcecd
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
  
  9 directories, 16 files                                         |  6 directories, 13 files
  [1]

Check that the diff that updates the submodule generates the correct delta
(i.e. instead of copying the entire working copy of the submodule every time)
  $ hg show --stat -T 'commit: {node}\n{desc}\n' .^
  commit: ebb49108081816413472ab4f46848efcb3ee9859
  Update submodule B in repo A
   smallrepofolder1/.x-repo-submodule-git-repo-b |  2 +-
   smallrepofolder1/.x-repo-submodule-repo_c     |  2 +-
   smallrepofolder1/git-repo-b/bar/zoo           |  1 -
   smallrepofolder1/git-repo-b/foo               |  1 -
   smallrepofolder1/git-repo-b/git-repo-c/choo3  |  1 +
   smallrepofolder1/git-repo-b/git-repo-c/choo4  |  1 +
   smallrepofolder1/repo_c/choo3                 |  1 +
   smallrepofolder1/repo_c/choo4                 |  1 +
   8 files changed, 6 insertions(+), 4 deletions(-)
  
  $ cat smallrepofolder1/.x-repo-submodule-git-repo-b
  0597690a839ce11a250139dae33ee85d9772a47a (no-eol)
