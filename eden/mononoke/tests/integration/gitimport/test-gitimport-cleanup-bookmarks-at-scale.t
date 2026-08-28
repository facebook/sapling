# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Cleanup at scale: 250 stale bookmarks span two full deletion batches plus a
# partial one (batch size 100), so this covers batch boundaries, the final
# short batch, and convergence -- the shape of a mirrored repo whose branch
# set was narrowed after thousands of refs had already been imported.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE

# Setup git repository: one commit, one long-lived branch, 250 disposable ones.
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ mkdir -p "$GIT_REPO"
  $ cd "$GIT_REPO"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ for i in $(seq -w 1 250); do git update-ref "refs/heads/scale/b$i" HEAD; done

# Import everything. 251 bookmarks: master_bookmark + the 250 scale branches.
  $ cd "$TESTTMP"
  $ gitimport "$GIT_REPO" --concurrency 100 --generate-bookmarks --suppress-ref-mapping full-repo 2>&1 | grep -c "(created)"
  251

# Drop every scale branch from git; master_bookmark stays.
  $ cd "$GIT_REPO"
  $ for i in $(seq -w 1 250); do git update-ref -d "refs/heads/scale/b$i"; done

# Cleanup deletes all 250 in batches, and only those.
  $ cd "$TESTTMP"
  $ gitimport "$GIT_REPO" --generate-bookmarks --suppress-ref-mapping --cleanup-mononoke-bookmarks full-repo 2>&1 | grep -c "(deleted)"
  250
  $ mononoke_admin bookmarks -R repo get heads/master_bookmark
  032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044
  $ mononoke_admin bookmarks -R repo get heads/scale/b001
  (not set)
  $ mononoke_admin bookmarks -R repo get heads/scale/b250
  (not set)

# A second cleanup run has nothing left to delete.
  $ gitimport "$GIT_REPO" --generate-bookmarks --suppress-ref-mapping --cleanup-mononoke-bookmarks full-repo 2>&1 | grep -c "(deleted)"
  0
  [1]
