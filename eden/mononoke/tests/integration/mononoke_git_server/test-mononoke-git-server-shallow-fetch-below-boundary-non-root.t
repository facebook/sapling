# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Test for shallow fetch: when a client with a shallow clone fetches a branch
# pointing to a non-root commit below the shallow boundary without specifying --depth,
# the fetch should still work.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"

# Setup git repository with a linear history of 5 commits
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
# Create first commit
  $ echo "content1" > file1
  $ git add .
  $ git commit -qam "commit1"
# Create second commit and capture its hash for later use
  $ echo "content2" > file2
  $ git add .
  $ git commit -qam "commit2"
  $ COMMIT2=$(git rev-parse HEAD)
# Create remaining commits
  $ echo "content3" > file3
  $ git add .
  $ git commit -qam "commit3"
  $ echo "content4" > file4
  $ git add .
  $ git commit -qam "commit4"
  $ echo "content5" > file5
  $ git add .
  $ git commit -qam "commit5"

# Visualize the commit graph
  $ git log --oneline
  a46a826 commit5
  888b3ec commit4
  914c86f commit3
  2f91057 commit2
  2aff56c commit1

# Import the repo into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO_ORIGIN" --derive-hg --generate-bookmarks full-repo

# Enable the JustKnobs for including heads in packfile commits and blocking indirect unshallow
  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:git_block_indirect_unshallow_fetch": true
  >   }
  > }
  > EOF

# Start up the Mononoke Git Service
  $ mononoke_git_service

# Shallow clone with depth=1 (only gets the tip commit - commit5)
  $ cd "$TESTTMP"
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --depth=1
  $ cd $REPONAME

# Verify we only have 1 commit (commit5)
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | grep commit | sort
  a46a826297ab026dcce385afe3917d0f9dadac2f commit 

# Verify the shallow file exists (confirms this is a shallow clone)
  $ test -f .git/shallow && echo "shallow clone confirmed"
  shallow clone confirmed

# Now create a branch on the origin pointing to the second commit (below shallow boundary, but not root)
  $ cd "$GIT_REPO_ORIGIN"
  $ git branch old_branch $COMMIT2
  $ git log old_branch --oneline
  2f91057 commit2
  2aff56c commit1

# Re-import to pick up the new branch
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO_ORIGIN" --generate-bookmarks full-repo

# Wait for the warm bookmark cache to catch up (need to be in the repo dir for git ls-remote)
  $ cd "$TESTTMP/$REPONAME"
  $ wait_for_git_bookmark_create "refs/heads/old_branch" 2>/dev/null

# Fetch the old_branch from the shallow clone
# This should fail: client is trying to fetch an ancestor of shallow boundary without --unshallow
  $ git_client fetch origin refs/heads/old_branch:refs/heads/old_branch 2>&1
  fatal: expected 'acknowledgments', received '?Failed to generate shallow info
  
  Caused by:
      You are indirectly trying to unshallow the repo without using unshallow or deepen argument. This can lead to broken repo state and hence is not supported. Please fetch with --unshallow argument instead.'
  [128]


# Verify fetch works with --unshallow
  $ git_client fetch --unshallow origin refs/heads/old_branch:refs/heads/old_branch 2>&1 | grep -v "^remote:"
  From https://localhost:$LOCAL_PORT/repos/git/ro/repo
   * [new branch]      old_branch -> old_branch

# Verify the commit was fetched and is accessible
  $ git log old_branch --oneline
  2f91057 commit2
  2aff56c commit1

# Verify we can read the file content from that commit
  $ git show old_branch:file2
  content2
