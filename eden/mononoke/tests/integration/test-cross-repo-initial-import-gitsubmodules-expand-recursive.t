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
# TODO(T174902563): set action to expand and submodule_dependencies
# $ set_git_submodules_action_in_config_version "$LATEST_CONFIG_VERSION_NAME" "$SMALL_REPO_ID" 3
# $ set_git_submodule_dependencies_in_config_version "$LATEST_CONFIG_VERSION_NAME" \
# > "$SMALL_REPO_ID" '{"git-repo-b": 3, "git-repo-c": 2}'
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
  `-- root_file
  
  5 directories, 7 files


  $ cd "$TESTTMP"


Import repos in reverse dependency order, C, B then A.

  $ REPOID="$REPO_C_ID" quiet gitimport "$GIT_REPO_C" full-repo

  $ REPOID="$REPO_B_ID" quiet gitimport "$GIT_REPO_B" full-repo

  $ REPOID="$SMALL_REPO_ID" with_stripped_logs gitimport "$GIT_REPO_A" full-repo > $TESTTMP/gitimport_output

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
  1aaa6e560792d9d1affcdb1538e083206b9fc3c8a535b058113e6c643ce022c5
  $ clone_and_log_large_repo "$SYNCED_HEAD"
  o  898ab49b1447 Added git repo B as submodule in A
  │   smallrepofolder1/.gitmodules |  3 +++
  │   1 files changed, 3 insertions(+), 0 deletions(-)
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
  
  RewrittenAs([(ChangesetId(Blake2(c33eeb91423c021a4d9d57f2efbb08185c77d89b9141433c666b84240395f0c5)), CommitSyncConfigVersion("INITIAL_IMPORT_SYNC_CONFIG"))])
  
  Deriving all the enabled derived data types

  $ HG_SYNCED_HEAD=$(mononoke_newadmin convert -R "$LARGE_REPO_NAME" -f bonsai -t hg "$SYNCED_HEAD")
  $ hg show --stat "$HG_SYNCED_HEAD"
  commit:      898ab49b1447
  user:        mononoke <mononoke@mononoke>
  date:        Sat Jan 01 00:00:00 2000 +0000
  files:       smallrepofolder1/.gitmodules
  description:
  Added git repo B as submodule in A
  
  
   smallrepofolder1/.gitmodules |  3 +++
   1 files changed, 3 insertions(+), 0 deletions(-)
  

  $ hg co -q "$HG_SYNCED_HEAD"

  $ tree
  .
  `-- smallrepofolder1
      |-- duplicates
      |   |-- x
      |   |-- y
      |   `-- z
      |-- regular_dir
      |   `-- aardvar
      `-- root_file
  
  3 directories, 5 files


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
  $ git log --oneline
  c9e2185 Update submodule C in repo B
  776166f Added git repo C as submodule in B
  b7dc5d8 Add bar/zoo
  1c7ecd4 Add foo

Update those changes in repo A
  $ cd $GIT_REPO_A
  $ # Make simple change directly in repo A
  $ echo "in A" >> root_file && git add .
  $ git commit -q -am "Change directly in A"
  $ # Update submodule b in A
  $ git submodule update --remote
  From $TESTTMP/git-repo-b
     776166f..c9e2185  master     -> origin/master
  Submodule path 'git-repo-b': checked out 'c9e218553071172339473b3cec7cc18dd5bcd978'
  $ git commit -q -am "Update submodule B in repo A" 
  $ git log --oneline
  13e2e77 Update submodule B in repo A
  f6c9619 Change directly in A
  f3ce0ee Added git repo B as submodule in A
  ad7b606 Add regular_dir/aardvar
  8c33a27 Add root_file


  $ REPOID="$REPO_C_ID" quiet gitimport "$GIT_REPO_C" full-repo

  $ REPOID="$REPO_B_ID" quiet gitimport "$GIT_REPO_B" full-repo

  $ REPOID="$SMALL_REPO_ID" with_stripped_logs gitimport "$GIT_REPO_A" full-repo > $TESTTMP/gitimport_output

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
  571eb1f42ab84f4a399434138d78a1b92fffb51ecd2716ffa18510f55ad8a8ed
  $ with_stripped_logs mononoke_newadmin derived-data -R "$LARGE_REPO_NAME" derive -i "$SYNCED_HEAD" -T hgchangesets
  $ HG_SYNCED_HEAD=$(mononoke_newadmin convert -R "$LARGE_REPO_NAME" -f bonsai -t hg "$SYNCED_HEAD")
  $ cd "$TESTTMP/$LARGE_REPO_NAME"
  $ hg pull -q -r "$HG_SYNCED_HEAD"
  $ hg co -q "$HG_SYNCED_HEAD"

  $ tree
  .
  `-- smallrepofolder1
      |-- duplicates
      |   |-- x
      |   |-- y
      |   `-- z
      |-- regular_dir
      |   `-- aardvar
      `-- root_file
  
  3 directories, 5 files
