# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Test edge cases for shallow clone depth parameter:
# 1. Depth exceeding history length (should return all commits)
# 2. Very large depth values
# 3. Depth=1 on a single commit repository
# 4. Depth=1 on a repository with only root commit

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"

# Setup git repository with 3 commits
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ git checkout -b master 2>/dev/null || true

  $ echo "content1" > file1
  $ git add .
  $ git commit -qam "C1: First commit"

  $ echo "content2" > file2
  $ git add .
  $ git commit -qam "C2: Second commit"

  $ echo "content3" > file3
  $ git add .
  $ git commit -qam "C3: Third commit"

# Import the repo into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO_ORIGIN" --derive-hg --generate-bookmarks full-repo

# Start up the Mononoke Git Service
  $ mononoke_git_service

# ============================================================
# TEST 1: Shallow clone with depth exceeding history length
# With only 3 commits, depth=10 should return all 3 commits
# ============================================================

  $ cd "$TESTTMP"
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --depth=10 depth_exceeds_history
  $ cd depth_exceeds_history

# Should have all 3 commits
  $ git rev-list --count HEAD
  3

# Verify we have all files
  $ ls file1 file2 file3
  file1
  file2
  file3

# The shallow file should NOT exist since we got the full history
  $ test -f .git/shallow && echo "shallow file exists" || echo "not shallow - full history"
  not shallow - full history

# ============================================================
# TEST 2: Shallow clone with very large depth value
# depth=999999 should effectively be a full clone
# ============================================================

  $ cd "$TESTTMP"
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --depth=999999 large_depth
  $ cd large_depth

# Should have all 3 commits
  $ git rev-list --count HEAD
  3

# The shallow file should NOT exist
  $ test -f .git/shallow && echo "shallow file exists" || echo "not shallow - full history"
  not shallow - full history

# ============================================================
# TEST 3: Shallow clone with depth=1 (normal case for reference)
# ============================================================

  $ cd "$TESTTMP"
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --depth=1 depth_one
  $ cd depth_one

# Should have exactly 1 commit
  $ git rev-list --count HEAD
  1

# Shallow file should exist
  $ test -f .git/shallow && echo "shallow clone confirmed"
  shallow clone confirmed

# But we should still have all the files from the tip
  $ ls file1 file2 file3
  file1
  file2
  file3

# ============================================================
# TEST 4: Compare Mononoke vs vanilla Git for depth exceeding history
# ============================================================

  $ cd "$TESTTMP"
  $ quiet git clone --depth=10 file://"$GIT_REPO_ORIGIN" vanilla_depth_exceeds

  $ cd vanilla_depth_exceeds
  $ VANILLA_COUNT=$(git rev-list --count HEAD)

  $ cd "$TESTTMP/depth_exceeds_history"
  $ MONONOKE_COUNT=$(git rev-list --count HEAD)

  $ test "$VANILLA_COUNT" = "$MONONOKE_COUNT" && echo "commit counts match: $MONONOKE_COUNT"
  commit counts match: 3

# ============================================================
# TEST 5: Shallow clone with depth=2 (middle of history)
# ============================================================

  $ cd "$TESTTMP"
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --depth=2 depth_two
  $ cd depth_two

# Should have exactly 2 commits
  $ git rev-list --count HEAD
  2

# Shallow file should exist
  $ test -f .git/shallow && echo "shallow clone confirmed"
  shallow clone confirmed

# Should have all files (working copy is complete)
  $ ls file1 file2 file3
  file1
  file2
  file3
