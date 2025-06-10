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


# Setup git repository
  $ quiet git_client clone -q $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ old_head=$(git rev-parse HEAD)
  $ git tag -a -m "incorrectly named tag" incorrect_tag
  $ mv .git/refs/tags/incorrect_tag .git/refs/incorrect_tag
  $ echo "this is file2" > file2
  $ git add file2
  $ git commit -qam "Add file2"

# Push Git repository to Mononoke
  $ git_client push origin refs/heads/master_bookmark:refs/heads/master_bookmark refs/incorrect_tag:refs/incorrect_tag
  To https://*/repos/git/ro/repo.git (glob)
   * [new branch]      master_bookmark -> master_bookmark
   * [new reference]   refs/incorrect_tag -> refs/incorrect_tag

  $ wait_for_git_bookmark_create refs/heads/master_bookmark

# Clone the pushed Git repository from Mononoke
  $ cd "$TESTTMP"
  $ quiet git_client clone --mirror -q $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git "$GIT_REPO"

  $ cd "$GIT_REPO"
  $ git show-ref
  e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/heads/master_bookmark
  6f5d55eb96433995aca8f272263ae2ea50e40ec7 refs/incorrect_tag
