# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Test common CI/CD workflow patterns with shallow clones.
# These are realistic scenarios that developers and CI systems encounter.
#
# Scenarios:
# 1. CI pattern: shallow clone, then fetch PR branch
# 2. Multiple fetches at same depth (idempotency)
# 3. Shallow clone with specific commit checkout
# 4. Shallow clone comparison with vanilla Git

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"

# Setup git repository simulating a real project
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ git checkout -b master 2>/dev/null || true

# Create initial project structure
  $ echo '{"name": "test-project", "version": "1.0.0"}' > package.json
  $ echo "# Test Project" > README.md
  $ mkdir -p src
  $ echo "console.log('hello');" > src/index.js
  $ git add .
  $ git commit -qam "Initial project setup"

# Add some history
  $ echo "function add(a, b) { return a + b; }" >> src/index.js
  $ git add .
  $ git commit -qam "Add math utilities"

  $ echo "function subtract(a, b) { return a - b; }" >> src/index.js
  $ git add .
  $ git commit -qam "Add subtract function"

  $ echo '{"name": "test-project", "version": "1.1.0"}' > package.json
  $ git add .
  $ git commit -qam "Bump version to 1.1.0"

  $ echo "## Features\n- Math utilities" >> README.md
  $ git add .
  $ git commit -qam "Update README with features"

# Create a feature branch (simulating a PR)
  $ git checkout -qb feature/new-multiply
  $ echo "function multiply(a, b) { return a * b; }" >> src/index.js
  $ git add .
  $ git commit -qam "Add multiply function"
  $ echo "function divide(a, b) { return a / b; }" >> src/index.js
  $ git add .
  $ git commit -qam "Add divide function"

# Go back to master
  $ git checkout -q master

# Import the repo into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO_ORIGIN" --derive-hg --generate-bookmarks full-repo

# Start up the Mononoke Git Service
  $ mononoke_git_service

# ============================================================
# TEST 1: CI pattern - shallow clone then fetch PR branch
# This is the most common CI workflow
# ============================================================

  $ cd "$TESTTMP"
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --depth=1 ci_checkout
  $ cd ci_checkout

# Verify shallow clone
  $ git rev-list --count HEAD
  1

# Fetch the feature branch (PR branch)
  $ git_client fetch origin refs/heads/feature/new-multiply:refs/heads/feature/new-multiply 2>&1 | grep -v "^remote:"
  From https://localhost:$LOCAL_PORT/repos/git/ro/repo
   * [new branch]      feature/new-multiply -> feature/new-multiply

# Checkout the feature branch
  $ git checkout -q feature/new-multiply

# Verify we can see the feature commits
  $ git log --oneline | head -2
  * Add divide function (glob)
  * Add multiply function (glob)

# Verify the code is there
  $ grep multiply src/index.js
  function multiply(a, b) { return a * b; }

# ============================================================
# TEST 2: Multiple fetches at same depth (idempotency)
# CI might fetch multiple times during a build
# ============================================================

  $ cd "$TESTTMP"
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --depth=1 multi_fetch
  $ cd multi_fetch

# First fetch
  $ git_client fetch 2>&1 | grep -E "error|fatal" || echo "fetch 1 OK"
  fetch 1 OK

# Second fetch (should be no-op)
  $ git_client fetch 2>&1 | grep -E "error|fatal" || echo "fetch 2 OK"
  fetch 2 OK

# Third fetch
  $ git_client fetch 2>&1 | grep -E "error|fatal" || echo "fetch 3 OK"
  fetch 3 OK

# Still have 1 commit
  $ git rev-list --count HEAD
  1

# ============================================================
# TEST 3: Shallow clone with specific commit checkout
# Some CIs need to checkout a specific commit
# ============================================================

  $ cd "$TESTTMP"
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --depth=10 specific_commit
  $ cd specific_commit

# Get a commit hash from history
  $ SPECIFIC_COMMIT=$(git log --oneline | grep "Add math" | cut -d' ' -f1)

# Checkout specific commit
  $ git checkout -q $SPECIFIC_COMMIT

# Verify we're at that commit
  $ git log --oneline -1
  * Add math utilities (glob)

# ============================================================
# TEST 4: Shallow clone comparison with vanilla Git
# Ensure Mononoke produces compatible output
# ============================================================

  $ cd "$TESTTMP"

# Vanilla Git shallow clone
  $ quiet git clone --depth=2 file://"$GIT_REPO_ORIGIN" vanilla_shallow

# Mononoke shallow clone
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --depth=2 mononoke_shallow

# Compare commit counts
  $ cd vanilla_shallow && VANILLA_COUNT=$(git rev-list --count HEAD) && cd ..
  $ cd mononoke_shallow && MONONOKE_COUNT=$(git rev-list --count HEAD) && cd ..

  $ test "$VANILLA_COUNT" = "$MONONOKE_COUNT" && echo "commit counts match"
  commit counts match

# Compare file content at HEAD
  $ diff <(cd vanilla_shallow && cat package.json) <(cd mononoke_shallow && cat package.json) && echo "package.json matches"
  package.json matches
