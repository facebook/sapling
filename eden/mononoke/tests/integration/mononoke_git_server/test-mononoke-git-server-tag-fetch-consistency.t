# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# This test verifies T257722899 fix: Tags created after initial import
# should be correctly included in subsequent fetches. This tests the
# RefsSource consistency between tag counting and tag resolution.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"

# Setup initial git repository with one tag
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ git tag -a -m "initial tag" v1.0

# Capture initial objects
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/initial_objects

# Import into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO_ORIGIN" --derive-hg --generate-bookmarks full-repo

# Start Mononoke Git Service
  $ mononoke_git_service

# First clone should work with initial tag
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git clone1
  $ cd clone1
  $ git tag -l
  v1.0

# Now add a NEW commit and tag to the origin repo
  $ cd "$GIT_REPO_ORIGIN"
  $ echo "this is file2" > file2
  $ git add file2
  $ git commit -qam "Add file2"
  $ git tag -a -m "second tag after initial import" v2.0

# Import the new changes into Mononoke
# This creates a tag that exists in the DB but may not be in WarmBookmarksCache immediately
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO_ORIGIN" --derive-hg --generate-bookmarks full-repo

# Fetch from Mononoke - this should include the new tag (v2.0)
# Before the T257722899 fix, this could fail with "bad object" error if the tag
# was in DB but not in WarmBookmarksCache.
# We use bypass-bookmark-cache to read from DB directly (since v2.0 isn't in WBC yet)
  $ cd clone1
  $ quiet git_client -c http.extraHeader="x-git-bypass-bookmark-cache: 1" fetch --tags origin

# Verify both tags are now present
  $ git tag -l | sort
  v1.0
  v2.0

# Create a fresh clone and verify it gets all tags
# Using bypass-bookmark-cache to ensure we see the newly imported tag from DB
  $ cd "$TESTTMP"
  $ quiet git_client -c http.extraHeader="x-git-bypass-bookmark-cache: 1" clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git clone2
  $ cd clone2
  $ git tag -l | sort
  v1.0
  v2.0

# Verify the objects match
  $ cd "$GIT_REPO_ORIGIN"
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/origin_objects
  $ cd "$TESTTMP/clone2"
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/clone2_objects
  $ diff -w $TESTTMP/clone2_objects $TESTTMP/origin_objects
