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

  $ cd "$TESTTMP"/mononoke-config
  $ cat >> repos/repo/server.toml <<EOF
  > [[bookmarks]]
  > regex=".*"
  > [[bookmarks.hooks]]
  > hook_name="block_accidental_new_bookmark_creation"
  > [[hooks]]
  > name="block_accidental_new_bookmark_creation"
  > config_json='''{
  > "allow_creations_with_marker": {
  >   "marker": "@new-branch",
  >   "comparison_prefix": "heads/"
  >  },
  >  "bypass_for_bookmarks_matching_regex": "^heads/prefix.*"
  > }'''
  > EOF
  $ cd -
  $TESTTMP


# Set Mononoke as the Source of Truth
  $ set_mononoke_as_source_of_truth_for_git

# Start up the Mononoke Git Service
  $ mononoke_git_service
# Clone the Git repo from Mononoke
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
  Cloning into 'repo'...

# Add some new commits to the cloned repo and push it to remote
  $ cd repo
  $ git checkout -b brand_new_branch
  Switched to a new branch 'brand_new_branch'
  $ echo new_file > new_file
  $ git add .
  $ git commit -qam "new commit"
  $ echo append >> new_file
  $ git add .
  $ git commit -qam "new commit"

# The git-receive-pack endpoint accepts pushes without moving the bookmarks in the backend
# but stores all the git and bonsai objects in the server
  $ git_client push origin brand_new_branch
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] brand_new_branch -> brand_new_branch (hooks failed:
    block_accidental_new_bookmark_creation for 2307d182492a0467ac583fa1135517ebd45a2615: Add "@new-branch: brand_new_branch" to the commit message to be able to create this branch.
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]

# change commit message
  $ git commit --amend -m "@new-branch: brand_new_branch and rest of the message here"
  [brand_new_branch 691e02c] @new-branch: brand_new_branch and rest of the message here
   Date: Sat Jan 1 00:00:00 2000 +0000
   1 file changed, 1 insertion(+)

# now push succeeds
  $ git_client push origin brand_new_branch
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   * [new branch]      brand_new_branch -> brand_new_branch

# now test the bypass based on bookmark matching a regex
  $ git checkout -b prefix_should_land_as_is
  Switched to a new branch 'prefix_should_land_as_is'
  $ echo a_file > a_file
  $ git add .
  $ git commit -qam "new commit"
  $ git_client push origin prefix_should_land_as_is
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   * [new branch]      prefix_should_land_as_is -> prefix_should_land_as_is

# now test that a new branch pointing at an existing commit should also fail
  $ git switch -c different_new_branch
  Switched to a new branch 'different_new_branch'
  $ echo a_file > a_file
  $ git add .
  $ git commit -qam "new commit"
  On branch different_new_branch
  nothing to commit, working tree clean
  [1]
  $ git_client push origin different_new_branch
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] different_new_branch -> different_new_branch (hooks failed:
    block_accidental_new_bookmark_creation for 4fe07c27b4b62e3d5168b4f7fd5863265af9d25e: Add "@new-branch: different_new_branch" to the commit message to be able to create this branch.
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]


