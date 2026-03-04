# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Test that --bypass-non-fast-forward allows gitimport to move bookmarks
# that are configured as fast-forward-only to non-descendant changesets.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ export ONLY_FAST_FORWARD_BOOKMARK="heads/master_bookmark"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO="${TESTTMP}/repo-git"

# Create a git repo with two commits on master_bookmark
  $ mkdir "$GIT_REPO"
  $ cd "$GIT_REPO"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ echo "this is file2" > file2
  $ git add file2
  $ git commit -qam "Add file2"

# Initial import: both commits imported, bookmark created
  $ cd "$TESTTMP"
  $ gitimport "$GIT_REPO" --generate-bookmarks full-repo
  [INFO] using repo "repo" repoid RepositoryId(0)
  [INFO] GitRepo:$TESTTMP/repo-git commit 2 of 2 - Oid:* => Bid:* (glob)
  [INFO] Ref: "refs/heads/master_bookmark": Some(ChangesetId(Blake2(*))) (glob)
  [INFO] Initializing repo: repo
  [INFO] Initialized repo: repo (1/1)
  [INFO] All repos initialized. It took: * seconds (glob)
  [INFO] Bookmark: "heads/master_bookmark": ChangesetId(Blake2(*)) (created) (glob)

# Now create a divergent commit: reset to file1, add file3 instead
  $ cd "$GIT_REPO"
  $ git reset --hard HEAD~1
  HEAD is now at * Add file1 (glob)
  $ echo "this is file3 (divergent)" > file3
  $ git add file3
  $ git commit -qam "Add file3 (divergent)"

# Re-import WITHOUT --bypass-non-fast-forward: should fail because
# heads/master_bookmark is configured as fast-forward-only
  $ cd "$TESTTMP"
  $ gitimport "$GIT_REPO" --generate-bookmarks full-repo
  [INFO] using repo "repo" repoid RepositoryId(0)
  [INFO] GitRepo:$TESTTMP/repo-git 1 of 2 commit(s) already exist (glob)
  * (glob)
  [INFO] Ref: "refs/heads/master_bookmark": Some(ChangesetId(Blake2(*))) (glob)
  [INFO] Initializing repo: repo
  [INFO] Initialized repo: repo (1/1)
  [INFO] All repos initialized. It took: * seconds (glob)
  [ERROR] Execution error: internal error: failed to move bookmark heads/master_bookmark from ChangesetId(Blake2(*)) to ChangesetId(Blake2(*)) (glob)
  
  Caused by:
      0: failed to move bookmark heads/master_bookmark from ChangesetId(Blake2(*)) to ChangesetId(Blake2(*)) (glob)
      1: failed to move bookmark heads/master_bookmark from ChangesetId(Blake2(*)) to ChangesetId(Blake2(*)) (glob)
  *Non fast-forward bookmark move* (glob)
  Error: Execution failed
  [1]


# Re-import WITH --bypass-non-fast-forward: should succeed
  $ gitimport "$GIT_REPO" --generate-bookmarks --bypass-non-fast-forward full-repo
  [INFO] using repo "repo" repoid RepositoryId(0)
  [INFO] GitRepo:$TESTTMP/repo-git 2 of 2 commit(s) already exist (glob)
  [INFO] Ref: "refs/heads/master_bookmark": Some(ChangesetId(Blake2(*))) (glob)
  [INFO] Initializing repo: repo
  [INFO] Initialized repo: repo (1/1)
  [INFO] All repos initialized. It took: * seconds (glob)
  [INFO] Bookmark: "heads/master_bookmark": ChangesetId(Blake2(*)) (moved from ChangesetId(Blake2(*))) (glob)
