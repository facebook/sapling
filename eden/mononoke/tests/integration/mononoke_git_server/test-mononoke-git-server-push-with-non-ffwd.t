# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ export ONLY_FAST_FORWARD_BOOKMARK_REGEX=".*ffonly"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
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
# Create another branch will allow non-fast-forward updates
  $ git checkout -b non_ffwd_branch
  Switched to a new branch 'non_ffwd_branch'
  $ echo "nonfwd file1" > nonfwdfile1
  $ git add nonfwdfile1
  $ git commit -qam "Add nonfwdfile1"
  $ initial_nonffwd_commit=$(git rev-parse HEAD)
  $ echo "nonfwd file2" > nonfwdfile2
  $ git add nonfwdfile2
  $ git commit -qam "Add nonfwdfile2"
# Create another branch that will be fast-forward only and we will try to delete it later
  $ git checkout -b branch_for_delete_ffonly
  Switched to a new branch 'branch_for_delete_ffonly'
  $ echo "delete file1" > delete_file1
  $ git add delete_file1
  $ git commit -qam "Add delete_file1"
# Create another branch that will be fast-forward only but we will bypass the restriction through pushvar
  $ git checkout -b bypass_branch_ffonly
  Switched to a new branch 'bypass_branch_ffonly'
  $ echo "bypass file1" > bypass_file1
  $ git add bypass_file1
  $ git commit -qam "Add bypass_file1"
  $ initial_bypass_commit=$(git rev-parse HEAD)
  $ echo "bypass file2" > bypass_file2
  $ git add bypass_file2
  $ git commit -qam "Add bypass_file2"
  $ git checkout master
  Switched to branch 'master'

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
# List all the known refs
  $ cd repo
  $ git show-ref | sort
  28718e2ecb4aec6de586603f3338f439f5b843ac refs/remotes/origin/bypass_branch_ffonly
  33f84db74b1f57fe45ae0fc29edc65ae984b979d refs/remotes/origin/non_ffwd_branch
  8963e1f55d1346a07c3aec8c8fc72bf87d0452b1 refs/tags/first_tag
  c47cf83db7aff6eb843f31a57d59f19670b69ed5 refs/remotes/origin/branch_for_delete_ffonly
  e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/heads/master
  e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/remotes/origin/HEAD
  e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/remotes/origin/master
  eb95862bb5d5c295844706cbb0d0e56fee405f5c refs/remotes/origin/branch_ffonly

# Add some new commits to the master branch
  $ echo "Just another file" > another_file
  $ git add .
  $ git commit -qam "Another commit on master"
# Try to do a non-ffwd push on branch_ffonly which should fail
  $ git checkout branch_ffonly
  Switched to a new branch 'branch_ffonly'
  ?ranch 'branch_ffonly' set up to track *branch_ffonly*. (glob)
  $ git reset --hard $initial_ffonly_commit
  HEAD is now at 3ea0687 Add fwdfile1
# Try doing a non-ffwd push on non_ffwd_branch branch which should succeed
  $ git checkout non_ffwd_branch
  Switched to a new branch 'non_ffwd_branch'
  ?ranch 'non_ffwd_branch' set up to track *non_ffwd_branch*. (glob)
  $ git reset --hard $initial_nonffwd_commit
  HEAD is now at 676bc3c Add nonfwdfile1
# Try doing a non-ffwd push on bypass_branch_ffonly branch which should normally fail
# but since we are providing a bypass, it should work
  $ git checkout bypass_branch_ffonly
  Switched to a new branch 'bypass_branch_ffonly'
  ?ranch 'bypass_branch_ffonly' set up to track *bypass_branch_ffonly*. (glob)
  $ git reset --hard $initial_bypass_commit
  HEAD is now at 7b248e9 Add bypass_file1

  $ git_client -c http.extraHeader="x-git-allow-non-ffwd-push: 1" push origin bypass_branch_ffonly --force
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   + 28718e2...7b248e9 bypass_branch_ffonly -> bypass_branch_ffonly (forced update)

# Try deleting a ffwd-only branch which should fail cause deletion is considered
# as a non-ffwd change
  $ git_client push origin --delete branch_for_delete_ffonly
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] branch_for_delete_ffonly (invalid request: Deletion of 'heads/branch_for_delete_ffonly' is prohibited)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]

# Push all the changes made so far
  $ git_client push origin master branch_ffonly non_ffwd_branch --force &> output
  [1]
  $ cat output | sort
     e8615d6..4981a25  master -> master
   ! [remote rejected] branch_ffonly -> branch_ffonly (Non fast-forward bookmark move of 'heads/branch_ffonly' from eb95862bb5d5c295844706cbb0d0e56fee405f5c to 3ea0687e31d7b65429c774526728dba90cbaabc0)
   + 33f84db...676bc3c non_ffwd_branch -> non_ffwd_branch (forced update)
  To https://*/repos/git/ro/repo.git (glob)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'

# Wait for the warm bookmark cache to catch up with the latest changes
  $ wait_for_git_bookmark_move HEAD $master_commit

# Clone the repo in a new folder
  $ cd "$TESTTMP"
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git new_repo
  Cloning into 'new_repo'...
  $ cd new_repo

# List all the known refs. Ensure that only master,non_ffwd_branch and bypass_branch_ffonly reflect a change
  $ git show-ref | sort
  4981a25180e49be096fce2ac3e68e455fc158449 refs/heads/master
  4981a25180e49be096fce2ac3e68e455fc158449 refs/remotes/origin/HEAD
  4981a25180e49be096fce2ac3e68e455fc158449 refs/remotes/origin/master
  676bc3cdd4bcc0b238223b6ca444c7ac50b59174 refs/remotes/origin/non_ffwd_branch
  7b248e999d14fbc53386479609031c21649c6598 refs/remotes/origin/bypass_branch_ffonly
  8963e1f55d1346a07c3aec8c8fc72bf87d0452b1 refs/tags/first_tag
  c47cf83db7aff6eb843f31a57d59f19670b69ed5 refs/remotes/origin/branch_for_delete_ffonly
  eb95862bb5d5c295844706cbb0d0e56fee405f5c refs/remotes/origin/branch_ffonly
