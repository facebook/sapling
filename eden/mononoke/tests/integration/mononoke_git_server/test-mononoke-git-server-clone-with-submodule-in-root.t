# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ ENABLED_DERIVED_DATA='["git_trees", "git_commits", "git_delta_manifests_v2", "skeleton_manifests"]' setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO_SUBMODULE="${TESTTMP}/origin/repo-submodule"
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ cat >> repos/repo/server.toml <<EOF
  > [source_control_service]
  > permit_writes = true
  > EOF

# Setup submodule git repository
  $ mkdir -p "$GIT_REPO_SUBMODULE"
  $ cd "$GIT_REPO_SUBMODULE"
  $ git init -q
  $ echo "this is submodule file1" > sub_file1
  $ git add sub_file1
  $ git commit -q -am "Add submodule file1"
  $ echo "this is submodule file2" > sub_file2
  $ git add sub_file2
  $ git commit -q -am "Add submodule file2"

# Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
# Add a submodule in this repository
  $ git submodule add "$GIT_REPO_SUBMODULE"
  Cloning into '$TESTTMP/origin/repo-git/repo-submodule'...
  done.
  $ git add .
  $ git commit -q -am "Add a new submodule"
# Add few regular commits
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ git tag -a -m"new tag" first_tag
  $ echo "this is file2" > file2
  $ git add file2
  $ git commit -qam "Add file2"
  $ git tag -a empty_tag -m ""
  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.

# Capture all the known Git objects from the repo
  $ cd $GIT_REPO
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/object_list

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO" --generate-bookmarks full-repo

# Start up the Mononoke Git Service
  $ mononoke_git_service
# Clone the Git repo from Mononoke
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git 
  Cloning into 'repo'...
# Verify that we get the same Git repo back that we started with
  $ cd $REPONAME  
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list
  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list  
