# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
Disable Mercurial types as they do not support git submodules
  $ DISABLED_DERIVED_DATA="filenodes hgchangesets hg_augmented_manifests" setup_common_config blob_files
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO_SUBMODULE="${TESTTMP}/origin/repo-submodule"
  $ GIT_REPO="${TESTTMP}/repo-git"

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
# Add few regular commits
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ git tag -a -m "new tag" first_tag
  $ echo "this is file2" > file2
  $ git add file2
  $ git commit -qam "Add file2"
  $ echo "this is file3" > file3
  $ git add file3
  $ git commit -qam "Add file3"
  $ git tag -a empty_tag -m ""
# Add a submodule in this repository
  $ git submodule add "$GIT_REPO_SUBMODULE"
  Cloning into '$TESTTMP/origin/repo-git/repo-submodule'...
  done.
  $ git add .
  $ git commit -q -am "Add a new submodule"
  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.

# Import the repo into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO_ORIGIN" --generate-bookmarks full-repo

# Start up the Mononoke Git Service
  $ mononoke_git_service

# Perform a shallow clone of the repo with depth = 2 and list the commits. This should work because the submodule is not
# present at this depth
  $ cd "$TESTTMP"
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --depth=2
  $ rm -rf $REPONAME

# Perform a shallow clone of the repo with depth = 1 and list the commits. This should fail because we try to fetch the
# size of the submodule
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --depth=1
