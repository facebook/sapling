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
  $ cat >> repos/repo/server.toml <<EOF
  > [source_control_service]
  > permit_writes = true
  > EOF

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
  > hook_name="block_new_bookmark_creations_by_name"
  > [[hooks]]
  > name="block_new_bookmark_creations_by_name"
  > config_json='''{
  > "blocked_bookmarks": ".*this_is_blocked.*"
  > }'''
  > bypass_pushvar="x-git-allow-all-bookmarks=1"
  > EOF
  $ cd "${TESTTMP}"

# Start up the Mononoke Git Service
  $ mononoke_git_service
# Clone the Git repo from Mononoke
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
  Cloning into 'repo'...

# Add some new commits to the cloned repo and push it to remote
  $ cd repo
  $ echo new_file > new_file
  $ git add .
  $ git commit -qam "Commit"

# This push works
  $ git_client push origin --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     e8615d6..8ff9b0a  master -> master

  $ git checkout -b this_is_blocked
  Switched to a new branch 'this_is_blocked'
  $ echo branc_new_file > brand_new_file
  $ git add .
  $ git commit -qam "Commit"

# This push is blocked
  $ git_client push origin --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] this_is_blocked -> this_is_blocked (hooks failed:
    block_new_bookmark_creations_by_name for 8643610acce921ac6df12107a4e671da0406984f: Creation of bookmark "heads/this_is_blocked" was blocked because it matched the '.*this_is_blocked.*' regular expression
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]

# This push succeeds
  $ git_client -c http.extraHeader="x-git-allow-all-bookmarks: 1" push origin --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   * [new branch]      this_is_blocked -> this_is_blocked
