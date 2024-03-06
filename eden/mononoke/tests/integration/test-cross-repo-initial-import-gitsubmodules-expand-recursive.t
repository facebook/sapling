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
  $ . "${TEST_FIXTURES}/library-xrepo-sync-with-git-submodules.sh"
  $ GIT_REPO_A="${TESTTMP}/git-repo-a"
  $ GIT_REPO_B="${TESTTMP}/git-repo-b"
  $ GIT_REPO_C="${TESTTMP}/git-repo-c"
  $ REPO_C_ID=2
  $ REPO_B_ID=3
  $ REPO_A_ID=$SMALL_REPO_ID

Avoid local clone error "fatal: transport 'file' not allowed" in new Git versions (see CVE-2022-39253).
  $ export XDG_CONFIG_HOME=$TESTTMP
  $ git config --global protocol.file.allow always


Run the x-repo with submodules setup  
  $ run_common_xrepo_sync_with_gitsubmodules_setup
  $ set_git_submodules_action_in_config_version "$LATEST_CONFIG_VERSION_NAME" "$SMALL_REPO_ID" 3
  $ set_git_submodule_dependencies_in_config_version "$LATEST_CONFIG_VERSION_NAME" \
  > "$SMALL_REPO_ID" '{"git-repo-b": 3, "git-repo-b/git-repo-c": 2, "repo_c": 2}'
  $ REPOID="$REPO_C_ID" REPONAME="repo_c" setup_common_config "$REPOTYPE"
  $ REPOID="$REPO_B_ID" REPONAME="repo_b" setup_common_config "$REPOTYPE"


Setup git repo C to be used as submodule in git repo B
  $ mkdir "$GIT_REPO_C"
  $ cd "$GIT_REPO_C"
  $ git init -q
  $ echo "choo" > choo
  $ git add choo
  $ git commit -q -am "Add choo"
  $ mkdir hoo
  $ cd hoo
  $ echo "qux" > qux
  $ cd ..
  $ git add hoo/qux
  $ git commit -q -am "Add hoo/qux"
  $ git log --oneline
  114b61c Add hoo/qux
  7f760d8 Add choo

Setup git repo B to be used as submodule in git repo A
  $ mkdir "$GIT_REPO_B"
  $ cd "$GIT_REPO_B"
  $ git init -q
  $ echo "foo" > foo
  $ git add foo
  $ git commit -q -am "Add foo"
  $ mkdir bar
  $ cd bar
  $ echo "zoo" > zoo
  $ cd ..
  $ git add bar/zoo
  $ git commit -q -am "Add bar/zoo"
  $ git submodule add ../git-repo-c
  Cloning into '$TESTTMP/git-repo-b/git-repo-c'...
  done.
  $ git add .
  $ git commit -q -am "Added git repo C as submodule in B" 
  $ git log --oneline
  776166f Added git repo C as submodule in B
  b7dc5d8 Add bar/zoo
  1c7ecd4 Add foo
  $ tree
  .
  |-- bar
  |   `-- zoo
  |-- foo
  `-- git-repo-c
      |-- choo
      `-- hoo
          `-- qux
  
  3 directories, 4 files


Setup git repo A
  $ mkdir "$GIT_REPO_A"
  $ cd "$GIT_REPO_A"
  $ git init -q
  $ echo "root_file" > root_file
  $ mkdir duplicates
  $ echo "Same content" > duplicates/x
  $ echo "Same content" > duplicates/y
  $ echo "Same content" > duplicates/z
  $ git add .
  $ git commit -q -am "Add root_file"
  $ mkdir regular_dir
  $ cd regular_dir
  $ echo "aardvar" > aardvar
  $ cd ..
  $ git add regular_dir/aardvar
  $ git commit -q -am "Add regular_dir/aardvar"
  $ git submodule add ../git-repo-b
  Cloning into '$TESTTMP/git-repo-a/git-repo-b'...
  done.
  $ git add .
  $ git commit -q -am "Added git repo B as submodule in A" 
  $ git log --oneline
  f3ce0ee Added git repo B as submodule in A
  ad7b606 Add regular_dir/aardvar
  8c33a27 Add root_file
  $ git submodule add ../git-repo-c repo_c
  Cloning into '$TESTTMP/git-repo-a/repo_c'...
  done.
  $ git add . && git commit -q -am "Added git repo C as submodule directly in A" 

  $ tree
  .
  |-- duplicates
  |   |-- x
  |   |-- y
  |   `-- z
  |-- git-repo-b
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
  
  7 directories, 9 files


  $ cd "$TESTTMP"


Import repos in reverse dependency order, C, B then A.

  $ REPOID="$REPO_C_ID" quiet gitimport "$GIT_REPO_C" --bypass-derived-data-backfilling full-repo
  $ REPOID="$REPO_B_ID" quiet gitimport "$GIT_REPO_B" --bypass-derived-data-backfilling full-repo
  $ REPOID="$SMALL_REPO_ID" with_stripped_logs gitimport "$GIT_REPO_A" --bypass-derived-data-backfilling full-repo > $TESTTMP/gitimport_output
  $ GIT_REPO_A_HEAD=$(rg ".*Ref: \"refs/heads/master\": Some\(ChangesetId\(Blake2\((\w+).+" -or '$1' $TESTTMP/gitimport_output)

  $ with_stripped_logs mononoke_x_repo_sync "$SMALL_REPO_ID" "$LARGE_REPO_ID" initial-import \
  > --no-progress-bar -i "$GIT_REPO_A_HEAD" \
  > --version-name "$LATEST_CONFIG_VERSION_NAME" 2>&1 | tee $TESTTMP/initial_import_output
  Starting session with id * (glob)
  Checking if * (glob)
  syncing * (glob)
  Found * unsynced ancestors (glob)
  changeset * synced as * in * (glob)
  successful sync of head * (glob)

  $ SYNCED_HEAD=$(rg ".+synced as (\w+) in.+" -or '$1' $TESTTMP/initial_import_output)
  $ echo $SYNCED_HEAD
  ed606b6cb39f87573b4613b88642ed01e89b728fe3620d6818a9fa842ea9409f
  $ clone_and_log_large_repo "$SYNCED_HEAD"
  o  cce5f6cbf89c Added git repo C as submodule directly in A
  │   smallrepofolder1/.gitmodules    |  3 +++
  │   smallrepofolder1/repo_c/choo    |  1 +
  │   smallrepofolder1/repo_c/hoo/qux |  1 +
  │   3 files changed, 5 insertions(+), 0 deletions(-)
  │
  o  661f5f6de455 Added git repo B as submodule in A
  │   smallrepofolder1/.gitmodules                   |  3 +++
  │   smallrepofolder1/git-repo-b/.gitmodules        |  3 +++
  │   smallrepofolder1/git-repo-b/bar/zoo            |  1 +
  │   smallrepofolder1/git-repo-b/foo                |  1 +
  │   smallrepofolder1/git-repo-b/git-repo-c/choo    |  1 +
  │   smallrepofolder1/git-repo-b/git-repo-c/hoo/qux |  1 +
  │   6 files changed, 10 insertions(+), 0 deletions(-)
  │
  o  e2c69ce8cc11 Add regular_dir/aardvar
  │   smallrepofolder1/regular_dir/aardvar |  1 +
  │   1 files changed, 1 insertions(+), 0 deletions(-)
  │
  o  df9086c77129 Add root_file
      smallrepofolder1/duplicates/x |  1 +
      smallrepofolder1/duplicates/y |  1 +
      smallrepofolder1/duplicates/z |  1 +
      smallrepofolder1/root_file    |  1 +
      4 files changed, 4 insertions(+), 0 deletions(-)
  
  
  
  Running mononoke_admin to verify mapping
  
  RewrittenAs([(ChangesetId(Blake2(eef414bd5fc8f7dcc129318276af6945117fe32bb5cfda6b0e6d43036107f61c)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
  
  Deriving all the enabled derived data types

  $ HG_SYNCED_HEAD=$(mononoke_newadmin convert -R "$LARGE_REPO_NAME" -f bonsai -t hg "$SYNCED_HEAD")
  $ hg show --stat -T 'commit: {node}\n{desc}\n' "$HG_SYNCED_HEAD"
  commit: cce5f6cbf89c890905f5b67cf1c605deb969fb2e
  Added git repo C as submodule directly in A
   smallrepofolder1/.gitmodules    |  3 +++
   smallrepofolder1/repo_c/choo    |  1 +
   smallrepofolder1/repo_c/hoo/qux |  1 +
   3 files changed, 5 insertions(+), 0 deletions(-)
  

  $ hg co -q "$HG_SYNCED_HEAD"

  $ tree | tee ${TESTTMP}/repo_a_tree_1
  .
  `-- smallrepofolder1
      |-- duplicates
      |   |-- x
      |   |-- y
      |   `-- z
      |-- git-repo-b
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
  
  9 directories, 11 files


Make changes to submodule and make sure they're synced properly

Make changes to repo C
  $ cd $GIT_REPO_C
  $ echo 'another file' > choo3 && git add .
  $ git commit -q -am "commit #3 in repo C" 
  $ echo 'another file' > choo4 && git add .
  $ git commit -q -am "commit #4 in repo C" 
  $ git log --oneline
  810d4f5 commit #4 in repo C
  55e8308 commit #3 in repo C
  114b61c Add hoo/qux
  7f760d8 Add choo

Update those changes in repo B
  $ cd $GIT_REPO_B
  $ git submodule update --remote
  From $TESTTMP/git-repo-c
     114b61c..810d4f5  master     -> origin/master
  Submodule path 'git-repo-c': checked out '810d4f53650b0fd891ad367ccfd8fa6067d93937'
  $ git add .
  $ git commit -q -am "Update submodule C in repo B" 
  $ rm bar/zoo foo
  $ git add . && git commit -q -am "Delete files in repo B" 
  $ git log --oneline
  0597690 Delete files in repo B
  c9e2185 Update submodule C in repo B
  776166f Added git repo C as submodule in B
  b7dc5d8 Add bar/zoo
  1c7ecd4 Add foo

Update those changes in repo A
  $ cd $GIT_REPO_A
  $ # Make simple change directly in repo A
  $ echo "in A" >> root_file && git add .
  $ git commit -q -am "Change directly in A"
Update submodule b in A
  $ git submodule update --remote
  From $TESTTMP/git-repo-b
     776166f..0597690  master     -> origin/master
  Submodule path 'git-repo-b': checked out '0597690a839ce11a250139dae33ee85d9772a47a'
  From $TESTTMP/git-repo-c
     114b61c..810d4f5  master     -> origin/master
  Submodule path 'repo_c': checked out '810d4f53650b0fd891ad367ccfd8fa6067d93937'
  $ git commit -q -am "Update submodule B in repo A" 
Then delete repo C submodule used directly in repo A
  $ git submodule deinit --force repo_c
  Cleared directory 'repo_c'
  Submodule 'repo_c' (../git-repo-c) unregistered for path 'repo_c'
  $ git rm -r repo_c
  rm 'repo_c'
  $ git add . && git commit -q -am "Remove repo C submodule from repo A"
  $ git log --oneline
  6775096 Remove repo C submodule from repo A
  5f6b001 Update submodule B in repo A
  de77178 Change directly in A
  3a41dad Added git repo C as submodule directly in A
  f3ce0ee Added git repo B as submodule in A
  ad7b606 Add regular_dir/aardvar
  8c33a27 Add root_file


  $ REPOID="$REPO_C_ID" quiet gitimport --bypass-derived-data-backfilling "$GIT_REPO_C" full-repo
  $ REPOID="$REPO_B_ID" quiet gitimport --bypass-derived-data-backfilling "$GIT_REPO_B" full-repo
  $ REPOID="$SMALL_REPO_ID" with_stripped_logs gitimport "$GIT_REPO_A" --bypass-derived-data-backfilling full-repo > $TESTTMP/gitimport_output
  $ GIT_REPO_A_HEAD=$(rg ".*Ref: \"refs/heads/master\": Some\(ChangesetId\(Blake2\((\w+).+" -or '$1' $TESTTMP/gitimport_output)

# TODO(T174902563): set up live sync instead of initial-import
  $ with_stripped_logs mononoke_x_repo_sync "$SMALL_REPO_ID" "$LARGE_REPO_ID" initial-import \
  > --no-progress-bar -i "$GIT_REPO_A_HEAD" \
  > --version-name "$LATEST_CONFIG_VERSION_NAME" 2>&1 | tee $TESTTMP/initial_import_output
  Starting session with id * (glob)
  Checking if * (glob)
  syncing * (glob)
  Found * unsynced ancestors (glob)
  changeset * synced as * in * (glob)
  successful sync of head * (glob)
 
  $ SYNCED_HEAD=$(rg ".+synced as (\w+) in.+" -or '$1' $TESTTMP/initial_import_output)
  $ echo "$SYNCED_HEAD" 
  810a54fb9eca92f42b3473b91a91590535896978b2cfaba457647471e1878b95
  $ with_stripped_logs mononoke_newadmin derived-data -R "$LARGE_REPO_NAME" derive -i "$SYNCED_HEAD" -T hgchangesets
  $ HG_SYNCED_HEAD=$(mononoke_newadmin convert -R "$LARGE_REPO_NAME" -f bonsai -t hg "$SYNCED_HEAD")
  $ cd "$TESTTMP/$LARGE_REPO_NAME"
  $ hg pull -q -r "$HG_SYNCED_HEAD"
  $ hg co -q "$HG_SYNCED_HEAD"

Check that deletions were made properly, i.e. submodule in repo_c was entirely
deleted and the files deleted in repo B were deleted inside its copy.
  $ hg show --stat -T 'commit: {node}\n{desc}\n' .
  commit: a8d716d8af176467acac2c6550c15ec2bcc88221
  Remove repo C submodule from repo A
   smallrepofolder1/.gitmodules    |  3 ---
   smallrepofolder1/repo_c/choo    |  1 -
   smallrepofolder1/repo_c/choo3   |  1 -
   smallrepofolder1/repo_c/choo4   |  1 -
   smallrepofolder1/repo_c/hoo/qux |  1 -
   5 files changed, 0 insertions(+), 7 deletions(-)
  


TODO(T174902563): Fix deletion of submodules in EXPAND submodule action.
  $ tree &> ${TESTTMP}/repo_a_tree_2
  $ diff -y -t -T ${TESTTMP}/repo_a_tree_1 ${TESTTMP}/repo_a_tree_2
  .                                                                  .
  `-- smallrepofolder1                                               `-- smallrepofolder1
      |-- duplicates                                                     |-- duplicates
      |   |-- x                                                          |   |-- x
      |   |-- y                                                          |   |-- y
      |   `-- z                                                          |   `-- z
      |-- git-repo-b                                                     |-- git-repo-b
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
  
  9 directories, 11 files                                         |  6 directories, 9 files
  [1]

Check that the diff that updates the submodule generates the correct delta
(i.e. instead of copying the entire working copy of the submodule every time)
  $ hg show --stat -T 'commit: {node}\n{desc}\n' .^
  commit: d9422f15e34aa8c42fc1f8e750f766ba8a3baa94
  Update submodule B in repo A
   smallrepofolder1/git-repo-b/bar/zoo          |  1 -
   smallrepofolder1/git-repo-b/foo              |  1 -
   smallrepofolder1/git-repo-b/git-repo-c/choo3 |  1 +
   smallrepofolder1/git-repo-b/git-repo-c/choo4 |  1 +
   smallrepofolder1/repo_c/choo3                |  1 +
   smallrepofolder1/repo_c/choo4                |  1 +
   6 files changed, 4 insertions(+), 2 deletions(-)
  
