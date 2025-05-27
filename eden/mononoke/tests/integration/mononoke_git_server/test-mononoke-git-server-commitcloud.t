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

# Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"

  $ echo "this is file2" > file2
  $ git add file2
  $ git commit -qam "Add file2"

  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO" --derive-hg --generate-bookmarks full-repo

# Set Mononoke as the Source of Truth
  $ set_mononoke_as_source_of_truth_for_git

# Start up the Mononoke Git Service
  $ mononoke_git_service
# Clone the Git repo from Mononoke
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git

# Add some new commits to the cloned repo and push it to remote
  $ cd repo
  $ current_head=$(git rev-parse HEAD)
  $ echo "newly added file" > new_file
  $ git add .
  $ git commit -qam "Commit with newly added file"
  $ git checkout -b new_branch
  Switched to a new branch 'new_branch'
  $ COMMIT_A=$(git rev-parse HEAD)

  $ echo "new file on new branch" > another_new_file
  $ git add .
  $ git commit -qam "New commit on new branch"
  $ COMMIT_B=$(git rev-parse HEAD)


# Push all the changes made so far
  $ git_client push origin $COMMIT_A:refs/commitcloud/upload_1 $COMMIT_A:refs/commitcloud/upload_2 
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   * [new reference]   e8b927ed84aa5ab33aeada81770a2aaa94f111be -> refs/commitcloud/upload_1
   * [new reference]   e8b927ed84aa5ab33aeada81770a2aaa94f111be -> refs/commitcloud/upload_2
