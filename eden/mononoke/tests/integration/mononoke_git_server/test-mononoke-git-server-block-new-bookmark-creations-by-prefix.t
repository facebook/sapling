# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO_SUBMODULE="${TESTTMP}/origin/repo-submodule-git"
  $ GIT_REPO="${TESTTMP}/repo-git"

# Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ old_head=$(git rev-parse HEAD)
  $ git tag -a -m"new tag" first_tag
  $ echo "this is file2" > file2
  $ git add file2
  $ git commit -qam "Add file2"
  $ git tag -a empty_tag -m ""
  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO" --derive-hg --generate-bookmarks full-repo

# Set Mononoke as the Source of Truth
  $ set_mononoke_as_source_of_truth_for_git

  $ cd "$TESTTMP"/mononoke-config
  $ cat >> repos/repo/server.toml <<EOF
  > [[bookmarks]]
  > regex=".*"
  > [[bookmarks.hooks]]
  > hook_name="block_new_bookmark_creations_by_prefix"
  > [[hooks]]
  > name="block_new_bookmark_creations_by_prefix"
  > config_json='''{
  > }'''
  > bypass_pushvar="x-git-allow-invalid-bookmarks=1"
  > EOF
  $ cd "${TESTTMP}"

# Start up the Mononoke Git Service
  $ mononoke_git_service
# Clone the Git repo from Mononoke
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git

# Add some new commits to the cloned repo and push it to remote
  $ cd repo
  $ echo new_file > new_file
  $ git add .
  $ git commit -qam "Commit"

# This push works
  $ git_client push origin --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     e8615d6..8ff9b0a  master_bookmark -> master_bookmark

  $ echo brand_new_file > brand_new_file
  $ git add .
  $ git commit -qam "Commit"

# This push is blocked
  $ git_client push origin HEAD:master_bookmark/another_master
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] HEAD -> master_bookmark/another_master (hooks failed:
    block_new_bookmark_creations_by_prefix for f53155321de7df9aa68c3b4b418019e612f0fa4b: Creation of bookmark "heads/master_bookmark/another_master" was blocked because its path prefix "heads/master_bookmark" already exists as a bookmark
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]

# Create a new branch and push to it
  $ git checkout -b just/some/created/branch
  Switched to a new branch 'just/some/created/branch'
  $ echo new_content > new_content
  $ git add .
  $ git commit -qam "New content commit"
  $ git_client push origin HEAD:just/some/created/branch
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   * [new branch]      HEAD -> just/some/created/branch

# Try pushing a path prefix of the branch. This will fail
  $ echo more_content > more_content
  $ git add .
  $ git commit -qam "More new content"
  $ git_client push origin HEAD:just/some/created
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] HEAD -> just/some/created (hooks failed:
    block_new_bookmark_creations_by_prefix for 134d5c589615ac5e391391b82f46f3722f89c924: Creation of bookmark "heads/just/some/created" was blocked because it exists as a path prefix of an existing bookmark
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]
  $ git_client push origin HEAD:just
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] HEAD -> just (hooks failed:
    block_new_bookmark_creations_by_prefix for 134d5c589615ac5e391391b82f46f3722f89c924: Creation of bookmark "heads/just" was blocked because it exists as a path prefix of an existing bookmark
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]

# Try pushing a prefix of the branch that is not path-prefix. This should work
  $ echo yet_more_content > yet_more_content
  $ git add .
  $ git commit -qam "More new content"
  $ git_client push origin HEAD:just/some/cr
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   * [new branch]      HEAD -> just/some/cr
