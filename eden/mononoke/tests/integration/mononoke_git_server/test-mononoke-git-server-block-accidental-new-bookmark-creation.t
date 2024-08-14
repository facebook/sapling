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
  >  }
  > }'''
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

  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:run_hooks_on_additional_changesets": true
  >   }
  > }
  > EOF

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

# The git-receive-pack endpoint accepts pushes without moving the bookmarks in the backend
# but stores all the git and bonsai objects in the server
  $ git_client push origin --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] brand_new_branch -> brand_new_branch (hooks failed:
    block_accidental_new_bookmark_creation for 1f4e1649c9648f8d8a2fbc5d42bbc6dddd1404df: Add "@new-branch: brand_new_branch" to the commit message to be able to create this branch.
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]

# change commit message
  $ git commit --amend -m "@new-branch: brand_new_branch and rest of the message here"
  [brand_new_branch 8457a8b] @new-branch: brand_new_branch and rest of the message here
   Date: Sat Jan 1 00:00:00 2000 +0000
   1 file changed, 1 insertion(+)
   create mode 100644 new_file

# now push succeeds
  $ git_client push origin --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   * [new branch]      brand_new_branch -> brand_new_branch
