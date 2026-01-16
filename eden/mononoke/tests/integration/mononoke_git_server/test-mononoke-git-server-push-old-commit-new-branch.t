# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ GIT_REPO_ALT="${TESTTMP}/repo-git-alt"

# Start up the Mononoke Git Service
  $ mononoke_git_service

# Set Mononoke as the Source of Truth
  $ set_mononoke_as_source_of_truth_for_git

# https://$(mononoke_git_service_address)/repos/git/ro

# Setup git repository
  $ quiet git_client clone -q $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git "$GIT_REPO"
  $ cd "$GIT_REPO"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ old_head=$(git rev-parse HEAD)
  $ git tag -a -m "new tag" first_tag
  $ quiet dd if=/dev/urandom of=440_KB_file bs=1K count=440
  $ git add 440_KB_file
  $ git commit -qam "Commit with large file"
  $ git tag -a empty_tag -m ""

# Push Git repository to Mononoke
  $ git_client push origin --all
  To https://*/repos/git/ro/repo.git (glob)
   * [new branch]      master_bookmark -> master_bookmark

# Wait for the warm bookmark cache to catch up with the latest changes
  $ wait_for_git_bookmark_create refs/heads/master_bookmark

# Clone the repo in alternative location
  $ cd "$TESTTMP"
  $ quiet git_client clone -q $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git "$GIT_REPO_ALT"
  $ cd "$GIT_REPO_ALT"
  $ old_head=$(git rev-parse HEAD)

# Push another commit on master branch on the first cloned repo
  $ cd "$GIT_REPO"
  $ echo "this is file3" > file3
  $ git add file3
  $ git commit -qam "Add file3"
  $ quiet git_client push origin master_bookmark

# Wait for the warm bookmark cache to catch up with the latest changes
  $ wait_for_git_bookmark_move refs/heads/master_bookmark $current_head

# The alternative repo checkout will not have the latest commit. Let's create a git branch pointing
# to the latest commit known to the alternative repo
  $ cd "$GIT_REPO_ALT"
  $ git checkout -b new_branch HEAD
  Switched to a new branch 'new_branch'
# Let's push this branch to the server. The commit that the branch points to is already known to the server.
# So essentially this should be a no-op push. However, due to an implementation bug we still end up sending data
# to the server for that commit that the server already has.

# Git has a work around for this with the use of a custom header called "push.negotiate" which when set to true
# forces git to negotiate with the server to find the common base so it doesn't have to send the extra data. In the
# below case without the header, git would have sent 440 KB+ of data. With the header, it just sends 183 bytes.
  $ git_client -c push.negotiate=true  push origin new_branch --verbose
  Pushing to https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
  POST git-receive-pack (183 bytes)
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   * [new branch]      new_branch -> new_branch
  updating local tracking ref 'refs/remotes/origin/new_branch'
