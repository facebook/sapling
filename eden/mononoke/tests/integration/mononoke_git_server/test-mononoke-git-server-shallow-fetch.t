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

# Start up the Mononoke Git Service
  $ mononoke_git_service
# Shallow clone the Git repo from Mononoke at depth of 1
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --depth 1 --single-branch -b master_bookmark

# Fetch for latest commits while maintaining the same shallow depth. Since nothing was pushed, there should be no new commits to fetch
# But due to the current bug in shallow fetch implementation with --deepen, we end up fetching the same data as before
  $ cd $REPONAME
  $ git_client fetch -k --progress origin +refs/heads/master_bookmark:refs/remotes/origin/master_bookmark --depth 1 --no-tags --force  &> $TESTTMP/actual_fetch
  $ grep -q "Receiving objects" $TESTTMP/actual_fetch && echo true
  [1]
  $ git_client fetch --progress -k origin +refs/heads/master_bookmark:refs/remotes/origin/master_bookmark --depth 1 --no-tags --force  &> $TESTTMP/actual_fetch
  $ grep -q "Receiving objects" $TESTTMP/actual_fetch && echo true
  [1]
  $ git_client fetch --progress -k origin +refs/heads/master_bookmark:refs/remotes/origin/master_bookmark --depth 1 --no-tags --force  &> $TESTTMP/actual_fetch
  $ grep -q "Receiving objects" $TESTTMP/actual_fetch && echo true
  [1]
  $ git_client fetch --progress -k origin +refs/heads/master_bookmark:refs/remotes/origin/master_bookmark --depth 1 --no-tags --force  &> $TESTTMP/actual_fetch
  $ grep -q "Receiving objects" $TESTTMP/actual_fetch && echo true
  [1]
