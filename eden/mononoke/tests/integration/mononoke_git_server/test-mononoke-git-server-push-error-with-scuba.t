# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ export ENABLE_BOOKMARK_CACHE=1
  $ REPOTYPE="blob_files"
  $ export ONLY_FAST_FORWARD_BOOKMARK_REGEX=".*ffonly"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ SCUBA="$TESTTMP/scuba.json"

# Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ git tag -a -m "new tag" first_tag
  $ echo "this is file2" > file2
  $ git add file2
  $ git commit -qam "Add file2"
  $ master_commit=$(git rev-parse HEAD)
# Create another branch which will be fast-forward only and add a few commits to it
  $ git checkout -b branch_ffonly
  Switched to a new branch 'branch_ffonly'
  $ echo "fwd file1" > fwdfile1
  $ git add fwdfile1
  $ git commit -qam "Add fwdfile1"
  $ initial_ffonly_commit=$(git rev-parse HEAD)
  $ echo "fwd file2" > fwdfile2
  $ git add fwdfile2
  $ git commit -qam "Add fwdfile2"
  $ echo "fwd file3" > fwdfile3
  $ git add fwdfile3
  $ git commit -qam "Add fwdfile3"
  $ git checkout master_bookmark
  Switched to branch 'master_bookmark'
  $ cd "$TESTTMP"
  $ git clone --mirror "$GIT_REPO_ORIGIN" repo-git
  Cloning into bare repository 'repo-git'...
  done.

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO" --derive-hg --generate-bookmarks full-repo

# Set Mononoke as the Source of Truth
  $ set_mononoke_as_source_of_truth_for_git

# Start up the Mononoke Git Service
  $ mononoke_git_service
# Clone the Git repo from Mononoke
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
  Cloning into 'repo'...
  $ cd repo


# Add some new commits to the master_bookmark branch
  $ echo "Just another file" > another_file
  $ git add .
  $ git commit -qam "Another commit on master_bookmark"
# Try to do a non-ffwd push on branch_ffonly which should fail
  $ git checkout branch_ffonly
  Switched to a new branch 'branch_ffonly'
  ?ranch 'branch_ffonly' set up to track *branch_ffonly*. (glob)
  $ git reset --hard $initial_ffonly_commit
  HEAD is now at 3ea0687 Add fwdfile1

# Push all the changes made so far
  $ git_client push origin master_bookmark branch_ffonly --force
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     e8615d6..60fb9c7  master_bookmark -> master_bookmark
   ! [remote rejected] branch_ffonly -> branch_ffonly (Non fast-forward bookmark move of 'heads/branch_ffonly' from eb95862bb5d5c295844706cbb0d0e56fee405f5c to 3ea0687e31d7b65429c774526728dba90cbaabc0
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]

# Wait for the warm bookmark cache to catch up with the latest changes
  $ wait_for_git_bookmark_move HEAD $master_commit

# Verify the push validation errors got recorded in scuba
  $ jq -S .normal "$SCUBA" | grep validation
    "push_validation_errors": "refs/heads/branch_ffonly => Non fast-forward bookmark move of 'heads/branch_ffonly' from eb95862bb5d5c295844706cbb0d0e56fee405f5c to 3ea0687e31d7b65429c774526728dba90cbaabc0\n\nFor more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j\n",
