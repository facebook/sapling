# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Test shallow clone followed by fetching additional branches.
# This simulates CI patterns where you clone one branch then fetch others.
#
# Scenarios:
# 1. Shallow clone default branch, then fetch a feature branch
# 2. Shallow clone, then fetch a branch that extends from within shallow history
# 3. Shallow clone with --single-branch, then fetch additional branches

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"

# Setup git repository with multiple branches from the start
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ git checkout -b master 2>/dev/null || true

  $ echo "content1" > file1
  $ git add .
  $ git commit -qam "C1: First commit"
  $ COMMIT1=$(git rev-parse HEAD)

  $ echo "content2" > file2
  $ git add .
  $ git commit -qam "C2: Second commit"
  $ COMMIT2=$(git rev-parse HEAD)

  $ echo "content3" > file3
  $ git add .
  $ git commit -qam "C3: Third commit"
  $ COMMIT3=$(git rev-parse HEAD)

  $ echo "content4" > file4
  $ git add .
  $ git commit -qam "C4: Fourth commit"
  $ COMMIT4=$(git rev-parse HEAD)

  $ echo "content5" > file5
  $ git add .
  $ git commit -qam "C5: Fifth commit"
  $ COMMIT5=$(git rev-parse HEAD)

# Create feature branch from tip (C5)
  $ git checkout -qb feature_from_tip
  $ echo "feature content" > feature_file
  $ git add .
  $ git commit -qam "F1: Feature commit from tip"

# Create feature branch from C4 (one commit below tip)
  $ git checkout -q $COMMIT4
  $ git checkout -qb feature_from_c4
  $ echo "feature c4 content" > feature_c4_file
  $ git add .
  $ git commit -qam "FC4: Feature from C4"

# Go back to master
  $ git checkout -q master

# Visualize the graph
  $ git log --all --oneline --graph | head -10
  * * FC4: Feature from C4 (glob)
  | * * F1: Feature commit from tip (glob)
  | * * C5: Fifth commit (glob)
  |/  
  * * C4: Fourth commit (glob)
  * * C3: Third commit (glob)
  * * C2: Second commit (glob)
  * * C1: First commit (glob)

# Import the repo into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO_ORIGIN" --derive-hg --generate-bookmarks full-repo

# Start up the Mononoke Git Service
  $ mononoke_git_service

# ============================================================
# TEST 1: Shallow clone default branch, then fetch feature branch from tip
# The feature branch extends from C5 (our tip), so this should work
# ============================================================

  $ cd "$TESTTMP"
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --depth=2
  $ cd $REPONAME

# Verify we have exactly 2 commits on master
  $ git rev-list --count HEAD
  2

# Verify shallow file exists
  $ test -f .git/shallow && echo "shallow clone confirmed"
  shallow clone confirmed

# Initially we only have master
  $ git branch -a | grep feature || echo "no feature branches"
  no feature branches

# Fetch the feature branch that extends from tip
  $ git_client fetch origin refs/heads/feature_from_tip:refs/heads/feature_from_tip 2>&1 | grep -v "^remote:"
  From https://localhost:$LOCAL_PORT/repos/git/ro/repo
   * [new branch]      feature_from_tip -> feature_from_tip

# Verify we got the branch
  $ git branch | grep feature
    feature_from_tip

# Verify we can see the feature commit
  $ git log feature_from_tip --oneline | head -1
  * F1: Feature commit from tip (glob)

# Verify we can access the feature file
  $ git show feature_from_tip:feature_file
  feature content

# ============================================================
# TEST 2: Fetch a branch that forks from within our shallow history
# feature_from_c4 branches from C4, which is in our depth=2 history
# ============================================================

  $ git_client fetch origin refs/heads/feature_from_c4:refs/heads/feature_from_c4 2>&1 | grep -v "^remote:"
  From https://localhost:$LOCAL_PORT/repos/git/ro/repo
   * [new branch]      feature_from_c4 -> feature_from_c4

# Verify we got the branch
  $ git branch | grep feature_from_c4
    feature_from_c4

# Verify we can see the feature commit
  $ git log feature_from_c4 --oneline | head -1
  * FC4: Feature from C4 (glob)

# Verify we can access the feature file
  $ git show feature_from_c4:feature_c4_file
  feature c4 content

# ============================================================
# TEST 3: Shallow clone with --single-branch, then fetch all
# ============================================================

  $ cd "$TESTTMP"
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --depth=2 --single-branch single_then_all
  $ cd single_then_all

# Initially only master
  $ git branch -a | grep -c origin
  2

# Fetch all branches explicitly
  $ git_client fetch origin refs/heads/feature_from_c4:refs/heads/feature_from_c4 2>&1 | grep -v "^remote:"
  From https://localhost:$LOCAL_PORT/repos/git/ro/repo
   * [new branch]      feature_from_c4 -> feature_from_c4
  $ git_client fetch origin refs/heads/feature_from_tip:refs/heads/feature_from_tip 2>&1 | grep -v "^remote:"
  From https://localhost:$LOCAL_PORT/repos/git/ro/repo
   * [new branch]      feature_from_tip -> feature_from_tip

# Now we have the branches
  $ git branch | wc -l
  3

# ============================================================
# TEST 4: Count total branches after all fetches in original clone
# ============================================================

  $ cd "$TESTTMP/$REPONAME"
  $ git branch | wc -l
  3
