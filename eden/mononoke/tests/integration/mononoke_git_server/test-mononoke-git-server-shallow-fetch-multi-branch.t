# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Test for shallow fetch with multiple branches, including diverged branches.
# This tests both linear history AND non-linear (diverged) branches:
#
# 1. Linear scenario: Multiple branches pointing to different commits in linear history
# 2. Diverged scenario: Branches that fork from the main line with their own commits
#
# Commit graph structure:
#
#   Main:     C1 -- C2 -- C3 -- C4 -- C5 -- C6 -- C7 (HEAD/master)
#              |     |     |     |
#              |     |     |     +-- mid_branch (points to C4)
#              |     |     +-------- ancient_branch (points to C3)
#              |     +-------------- older_branch (points to C2)
#              +-------------------- oldest_branch (points to C1)
#              |
#   Hotfix:   C1 -- H1 -- H2 (hotfix_branch, diverged from C1)
#                    |
#   Release:  C1 -- C2 -- C3 -- R1 -- R2 (release_branch, diverged from C3)
#
# With depth=1 shallow clone of main, client gets C7 only
# Tests verify fetching:
# 1. Linear branches pointing to various commits below the boundary
# 2. Diverged branches that have their own commits not in main's history

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"

# Setup git repository with linear history
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
# Force the initial branch to be called master for consistency
  $ git checkout -b master 2>/dev/null || true

# Create main branch with 7 commits
  $ echo "content1" > file1
  $ git add .
  $ git commit -qam "C1: Initial commit"
  $ COMMIT1=$(git rev-parse HEAD)

  $ echo "content2" > file2
  $ git add .
  $ git commit -qam "C2: Add file2"
  $ COMMIT2=$(git rev-parse HEAD)

  $ echo "content3" > file3
  $ git add .
  $ git commit -qam "C3: Add file3"
  $ COMMIT3=$(git rev-parse HEAD)

  $ echo "content4" > file4
  $ git add .
  $ git commit -qam "C4: Add file4"
  $ COMMIT4=$(git rev-parse HEAD)

  $ echo "content5" > file5
  $ git add .
  $ git commit -qam "C5: Add file5"
  $ COMMIT5=$(git rev-parse HEAD)

  $ echo "content6" > file6
  $ git add .
  $ git commit -qam "C6: Add file6"
  $ COMMIT6=$(git rev-parse HEAD)

  $ echo "content7" > file7
  $ git add .
  $ git commit -qam "C7: Add file7"
  $ COMMIT7=$(git rev-parse HEAD)

# Create linear branches pointing to different commits in the linear history
  $ git branch oldest_branch $COMMIT1
  $ git branch older_branch $COMMIT2
  $ git branch ancient_branch $COMMIT3
  $ git branch mid_branch $COMMIT4

# Create diverged hotfix branch from C1 with its own commits
# Use git checkout -b to create and switch to the branch
  $ git checkout -q master
  $ git checkout -q -b hotfix_branch $COMMIT1
  $ echo "hotfix1" > hotfix_file1
  $ git add .
  $ git commit -qam "H1: First hotfix"
  $ echo "hotfix2" > hotfix_file2
  $ git add .
  $ git commit -qam "H2: Second hotfix"
  $ HOTFIX_HEAD=$(git rev-parse HEAD)

# Create diverged release branch from C3 with its own commits
  $ git checkout -q master
  $ git checkout -q -b release_branch $COMMIT3
  $ echo "release1" > release_file1
  $ git add .
  $ git commit -qam "R1: First release commit"
  $ echo "release2" > release_file2
  $ git add .
  $ git commit -qam "R2: Second release commit"
  $ RELEASE_HEAD=$(git rev-parse HEAD)

# Go back to master to leave repo in clean state for gitimport
  $ git checkout -q master

# Verify we have correct branch structure
  $ git log --oneline master | wc -l
  7
  $ git log --oneline hotfix_branch | wc -l
  3
  $ git log --oneline release_branch | wc -l
  5

# Import the repo into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO_ORIGIN" --derive-hg --generate-bookmarks full-repo

# Enable the JustKnob for blocking indirect unshallow fetch
  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:git_block_indirect_unshallow_fetch": true
  >   }
  > }
  > EOF

# Start up the Mononoke Git Service
  $ mononoke_git_service

# Shallow clone with depth=1 (gets C7 only)
  $ cd "$TESTTMP"
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --depth=1
  $ cd $REPONAME

# Verify we have exactly 1 commit
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | grep commit | wc -l
  1

# Verify the shallow file exists
  $ test -f .git/shallow && echo "shallow clone confirmed"
  shallow clone confirmed

# ============================================================
# TEST 1: Fetch 'oldest_branch' (points to C1, first commit)
# This tests fetching a branch that points to the very first commit.
# This should FAIL because C1 is an ancestor of the shallow boundary (C7),
# meaning the client is trying to indirectly unshallow the repo.
# ============================================================

  $ git_client fetch origin refs/heads/oldest_branch:refs/heads/oldest_branch 2>&1 | grep -v "^remote:"
  fatal: expected 'acknowledgments', received '?Failed to generate shallow info
  
  Caused by:
      You are indirectly trying to unshallow the repo without using unshallow or deepen argument. This can lead to broken repo state and hence is not supported. Please fetch with --unshallow argument instead.'


# ============================================================
# TEST 2: Fetch 'older_branch' (points to C2)
# This should FAIL for the same reason as TEST 1.
# ============================================================

  $ git_client fetch origin refs/heads/older_branch:refs/heads/older_branch 2>&1 | grep -v "^remote:"
  fatal: expected 'acknowledgments', received '?Failed to generate shallow info
  
  Caused by:
      You are indirectly trying to unshallow the repo without using unshallow or deepen argument. This can lead to broken repo state and hence is not supported. Please fetch with --unshallow argument instead.'


# ============================================================
# TEST 3: Fetch 'ancient_branch' (points to C3)
# This should FAIL for the same reason as TEST 1.
# ============================================================

  $ git_client fetch origin refs/heads/ancient_branch:refs/heads/ancient_branch 2>&1 | grep -v "^remote:"
  fatal: expected 'acknowledgments', received '?Failed to generate shallow info
  
  Caused by:
      You are indirectly trying to unshallow the repo without using unshallow or deepen argument. This can lead to broken repo state and hence is not supported. Please fetch with --unshallow argument instead.'


# ============================================================
# TEST 4: Fetch 'mid_branch' (points to C4)
# This should FAIL for the same reason as TEST 1.
# ============================================================

  $ git_client fetch origin refs/heads/mid_branch:refs/heads/mid_branch 2>&1 | grep -v "^remote:"
  fatal: expected 'acknowledgments', received '?Failed to generate shallow info
  
  Caused by:
      You are indirectly trying to unshallow the repo without using unshallow or deepen argument. This can lead to broken repo state and hence is not supported. Please fetch with --unshallow argument instead.'


# ============================================================
# TEST 5: Fetch 'hotfix_branch' (diverged from C1 with H1, H2)
# This tests fetching a diverged branch that forked from the first commit
# and has its own commits not in the main line.
# This should FAIL because fetching this branch would require sending
# commits (C1, H1, H2) where C1 is below the shallow boundary. Even though
# the branch diverges, it still requires commits the client doesn't have
# that are ancestors of the shallow boundary.
# ============================================================

  $ git_client fetch origin refs/heads/hotfix_branch:refs/heads/hotfix_branch 2>&1 | grep -v "^remote:"
  fatal: expected 'acknowledgments', received '?Failed to generate shallow info
  
  Caused by:
      You are indirectly trying to unshallow the repo without using unshallow or deepen argument. This can lead to broken repo state and hence is not supported. Please fetch with --unshallow argument instead.'


# ============================================================
# TEST 6: Fetch 'release_branch' (diverged from C3 with R1, R2)
# This tests fetching a diverged branch that forked from C3
# and has its own commits not in the main line.
# This should FAIL for the same reason as TEST 5 - it requires
# commits (C1, C2, C3) below the shallow boundary.
# ============================================================

  $ git_client fetch origin refs/heads/release_branch:refs/heads/release_branch 2>&1 | grep -v "^remote:"
  fatal: expected 'acknowledgments', received '?Failed to generate shallow info
  
  Caused by:
      You are indirectly trying to unshallow the repo without using unshallow or deepen argument. This can lead to broken repo state and hence is not supported. Please fetch with --unshallow argument instead.'


# ============================================================
# TEST 7: Summary
# All branches (linear and diverged) were blocked by validation because
# they all require commits that are below the shallow boundary.
# ============================================================

# Verify we still only have the original branch from the shallow clone
  $ git branch | wc -l
  1
