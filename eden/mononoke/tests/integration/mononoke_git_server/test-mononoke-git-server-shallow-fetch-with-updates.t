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
  $ git tag -a -m"new tag" first_tag
  $ echo "this is file2" > file2
  $ git add file2  
  $ git commit -qam "Add file2"
  $ echo "this is file3" > file3
  $ git add file3 
  $ git commit -qam "Add file3"
  $ echo "this is file4" > file4
  $ git add file4
  $ git commit -qam "Add file4"  
  $ echo "this is file5" > file5
  $ git add file5
  $ git commit -qam "Add file5"  
  $ git tag -a empty_tag -m ""
  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.

# Capture all the known Git objects from the repo
  $ cd $GIT_REPO
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/object_list

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO" --derive-hg --generate-bookmarks full-repo

# Set Mononoke as the Source of Truth
  $ set_mononoke_as_source_of_truth_for_git

# Start up the Mononoke Git Service
  $ mononoke_git_service
# Shallow clone the Git repo from Mononoke at depth of 1
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --depth 1 --single-branch -b master_bookmark

# Fetch for latest commits while maintaining the same shallow depth. Since nothing was pushed, there should be no new commits to fetch
  $ cd $REPONAME
  $ git log | head -n 1
  commit 1b45cf3ca36a5feea675bf45b5fc6e9abb160886
  $ git_client fetch -k --progress origin +refs/heads/master_bookmark:refs/remotes/origin/master_bookmark --depth 1 --no-tags --force  &> $TESTTMP/actual_fetch
  $ grep -q "Receiving objects" $TESTTMP/actual_fetch && echo true
  [1]

# Push a new commit to the repo
  $ cd "$TESTTMP"
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git push_repo
  $ cd push_repo
  $ current_head=$(git rev-parse HEAD)
  $ echo "this is file6" > file6
  $ git add file6
  $ git commit -qam "Add file6"
  $ git_client push -q origin master_bookmark

# Wait for the warm bookmark cache to catch up with the latest changes
  $ wait_for_git_bookmark_move HEAD $current_head

# Fetch for latest commits while maintaining the same shallow depth. Since there was a new commit, we should see data being fetched
  $ cd "$TESTTMP"/$REPONAME
  $ git_client fetch --progress -k origin +refs/heads/master_bookmark:refs/remotes/origin/master_bookmark --depth 1 --no-tags --force  &> $TESTTMP/actual_fetch
  $ grep -q "Receiving objects" $TESTTMP/actual_fetch && echo true
  true

  $ git log | head -n 1
  commit 1b45cf3ca36a5feea675bf45b5fc6e9abb160886

# Push another new commit to the repo
  $ cd "$TESTTMP"
  $ rm -rf push_repo
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git push_repo
  $ cd push_repo
  $ current_head=$(git rev-parse HEAD)
  $ echo "this is file7" > file7
  $ git add file7
  $ git commit -qam "Add file7"
  $ git_client push -q origin master_bookmark

# Wait for the warm bookmark cache to catch up with the latest changes
  $ wait_for_git_bookmark_move HEAD $current_head

# Fetch for latest commits while also increasing the shallow depth. We should get new commits at both ends (near the head and near the shallow boundary)
  $ cd $TESTTMP/$REPONAME
  $ git_client fetch --progress -k origin +refs/heads/master_bookmark:refs/remotes/origin/master_bookmark --depth 1 --no-tags --force  &> $TESTTMP/actual_fetch
  $ grep -q "Receiving objects" $TESTTMP/actual_fetch && echo true
  true

  $ git log | head -n 1
  commit 1b45cf3ca36a5feea675bf45b5fc6e9abb160886
