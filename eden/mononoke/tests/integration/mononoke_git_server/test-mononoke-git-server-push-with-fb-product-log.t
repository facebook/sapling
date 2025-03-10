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
  $ quiet git_client -c http.extraHeader="x-fb-product-log: git:123:pid1234_1234" clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
  $ cd repo

# Add some new commits to the master_bookmark branch
  $ echo "Just another file" > another_file
  $ git add .
  $ git commit -qam "Another commit on master_bookmark"

# Push all the changes made so far
  $ git_client -c http.extraHeader="x-fb-product-log: git:123:pid1234_5678" push origin master_bookmark
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     e8615d6..60fb9c7  master_bookmark -> master_bookmark

# Wait for the warm bookmark cache to catch up with the latest changes
  $ wait_for_git_bookmark_move HEAD $master_commit

# Verify the push validation errors got recorded in scuba
  $ jq -S .normal "$SCUBA" | grep product | wc -l
  38
