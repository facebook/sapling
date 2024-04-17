# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Integration test based on scenario: https://internalfb.com/excalidraw/EX192102
  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ ENABLED_DERIVED_DATA='["git_commits", "git_trees", "git_delta_manifests", "unodes", "filenodes", "hgchangesets"]' setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ HG_REPO="${TESTTMP}/repo-hg"
  $ BUNDLE_PATH="${TESTTMP}/repo_bundle.bundle"
  $ cat >> repos/repo/server.toml <<EOF
  > [source_control_service]
  > permit_writes = true
  > EOF

# Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
# Add a base commit on master
  $ echo "this is origin" > origin
  $ git add origin
  $ git commit -qam "Add origin"
# Create branch R1 from master and add three commits on it
  $ git checkout -qb R1
  $ echo "this is file1.this is file1.this is file1.this is file1.this is file1.this is file1.this is file1.this is file1.this is file1.this is file1.this is file1.this is file1." > file1
  $ git add file1
  $ git commit -qam "Add file1 in branch R1"
  $ echo "this is file1.1.this is file1.1.this is file1.1.this is file1.1.this is file1.1.this is file1.1.this is file1.1.this is file1.1.this is file1.1.this is file1.1.this is file1.1." > file1
  $ git add file1
  $ git commit -qam "Modified file1 -> file1.1 in branch R1"
  $ echo "this is file1.2.this is file1.2.this is file1.2.this is file1.2.this is file1.2.this is file1.2.this is file1.2.this is file1.2.this is file1.2.this is file1.2.this is file1.2." > file1
  $ git add file1
  $ git commit -qam "Modified file1.1 -> file1.2 in branch R1"
# Create branch R2 from master and add two commits on it
  $ git checkout -qb R2 master
  $ echo "this is file1.1.this is file1.1.this is file1.1.this is file1.1.this is file1.1.this is file1.1.this is file1.1.this is file1.1.this is file1.1.this is file1.1.this is file1.1." > file1
  $ git add file1
  $ git commit -qam "Add file1.1 in branch R2"
  $ echo "this is file1.this is file1.this is file1.this is file1.this is file1.this is file1.this is file1.this is file1.this is file1.this is file1.this is file1.this is file1." > file1
  $ git add file1
  $ git commit -qam "Modified file1.1 -> file1 in branch R2"

  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.

# Capture all the known Git objects from the repo
  $ cd $GIT_REPO
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/object_list

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO" --derive-hg --generate-bookmarks full-repo

# Start up the Mononoke Git Service
  $ mononoke_git_service
# Clone the Git repo from Mononoke
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
  Cloning into 'repo'...
# Verify that we get the same Git repo back that we started with
  $ cd $REPONAME  
  $ current_head=$(git rev-parse HEAD)
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list
  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list

# Add more commits to the original git repo
  $ cd $GIT_REPO_ORIGIN
  $ git checkout -q R1
  $ echo "this is file1.3.this is file1.3.this is file1.3.this is file1.3.this is file1.3.this is file1.3.this is file1.3.this is file1.3.this is file1.3.this is file1.3.this is file1.3." > file1
  $ git add file1
  $ git commit -qam "Modified file1.2 -> file1.3 in branch R1"
  $ echo "this is file1.this is file1.this is file1.this is file1.this is file1.this is file1.this is file1.this is file1.this is file1.this is file1.this is file1.this is file1." > file1
  $ git add file1
  $ git commit -qam "Modified file1.3 -> file1 in branch R1"

  $ git checkout -q R2
  $ echo "this is file1.3.this is file1.3.this is file1.3.this is file1.3.this is file1.3.this is file1.3.this is file1.3.this is file1.3.this is file1.3.this is file1.3.this is file1.3." > file1
  $ git add file1
  $ git commit -qam "Modified file1 -> file1.3 in branch R2"
  $ echo "this is file1.2.this is file1.2.this is file1.2.this is file1.2.this is file1.2.this is file1.2.this is file1.2.this is file1.2.this is file1.2.this is file1.2.this is file1.2." > file1
  $ git add file1
  $ git commit -qam "Modified file1.3 -> file1.2 in branch R2"

  $ cd "$GIT_REPO"
  $ quiet git pull "$GIT_REPO_ORIGIN"
# Capture all the known Git objects from the repo
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/object_list

# Import the newly added commits to Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO_ORIGIN" --derive-hg --generate-bookmarks full-repo
# Pull the Git repo from Mononoke
  $ cd $REPONAME
# Wait for the warm bookmark cache to catch up with the latest changes
  $ wait_for_git_bookmark_move HEAD $current_head
  $ quiet git_client pull $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
# Verify that we get the same Git repo back that we started with
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list
  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list
