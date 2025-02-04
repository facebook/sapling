# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"

# Start up the Mononoke Git Service
  $ mononoke_git_service

# Set Mononoke as the Source of Truth
  $ set_mononoke_as_source_of_truth_for_git

# https://$(mononoke_git_service_address)/repos/git/ro

# Setup git repository
  $ quiet git_client clone -q $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ old_head=$(git rev-parse HEAD)
  $ git tag -a -m "new tag" first_tag
  $ echo "this is file2" > file2
  $ git add file2
  $ git commit -qam "Add file2"
  $ git tag -a empty_tag -m ""

# Push Git repository to Mononoke
  $ git_client push origin --all
  To https://*/repos/git/ro/repo.git (glob)
   * [new branch]      master_bookmark -> master_bookmark
