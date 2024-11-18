# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ ENABLED_DERIVED_DATA='["git_trees", "filenodes", "hgchangesets"]' setup_common_config
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ HG_REPO="${TESTTMP}/repo"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE

# Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ git tag -a -m"new tag" first_tag
  $ git tag -a -m "changing tag" changing_tag
  $ git checkout -b "another_branch"
  Switched to a new branch 'another_branch'
  $ git checkout master_bookmark
  Switched to branch 'master_bookmark'
  $ tagged_commit=$(git rev-parse HEAD)
# Create a recursive tag to check if it gets imported
  $ git config advice.nestedTag false
  $ git tag -a recursive_tag -m "this recursive tag points to first_tag" $(git rev-parse first_tag)
  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.
  $ cd repo-git
  $ git fetch "$GIT_REPO_ORIGIN" +refs/*:refs/* --prune -u
  From $TESTTMP/origin/repo-git
   - [deleted]         (none)         -> origin/another_branch
   - [deleted]         (none)         -> origin/master_bookmark
     (refs/remotes/origin/HEAD has become dangling)
   * [new branch]      another_branch -> another_branch
  $ git branch "a_ref_prefixed_by_remotes_origin"
  $ git update-ref refs/remotes/origin/a_ref_prefixed_by_remotes_origin a_ref_prefixed_by_remotes_origin
  $ git branch -d a_ref_prefixed_by_remotes_origin
  Deleted branch a_ref_prefixed_by_remotes_origin (was 8ce3eae).
  $ cd ..


# Import it into Mononoke. Note: cleanup-mononoke-bookmarks does nothing here, but we want to show that it doesn't.
  $ cd "$TESTTMP"
  $ with_stripped_logs gitimport "$GIT_REPO" --concurrency 100 --generate-bookmarks --cleanup-mononoke-bookmarks full-repo
  using repo "repo" repoid RepositoryId(0)
  GitRepo:$TESTTMP/repo-git commit 1 of 1 - Oid:8ce3eae4 => Bid:032cd4dc
  Ref: "refs/heads/another_branch": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Ref: "refs/heads/master_bookmark": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Ref: "refs/remotes/origin/a_ref_prefixed_by_remotes_origin": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Ref: "refs/tags/changing_tag": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Ref: "refs/tags/first_tag": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Ref: "refs/tags/recursive_tag": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Initializing repo: repo
  Initialized repo: repo
  All repos initialized. It took: 0 seconds
  Bookmark: "heads/another_branch": ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)) (created)
  Bookmark: "heads/master_bookmark": ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)) (created)
  Bookmark: "remotes/origin/a_ref_prefixed_by_remotes_origin": ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)) (created)
  Bookmark: "tags/changing_tag": ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)) (created)
  Bookmark: "tags/first_tag": ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)) (created)
  Bookmark: "tags/recursive_tag": ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)) (created)


# Delete some refs from the git repo
  $ cd "$GIT_REPO"
  $ git tag --delete first_tag
  Deleted tag 'first_tag' (was 8963e1f)
  $ git push --delete origin refs/tags/first_tag
  To $TESTTMP/origin/repo-git
   - [deleted]         first_tag
  $ git branch --delete another_branch
  Deleted branch another_branch (was 8ce3eae).
  $ git push --delete origin refs/heads/another_branch
  To $TESTTMP/origin/repo-git
   - [deleted]         another_branch

# For now, the tag and ref are still in Mononoke
  $ mononoke_admin bookmarks -R repo get tags/first_tag
  Metadata changeset for tag bookmark tags/first_tag: 
  5ca579c0e3ebea708371b65ce559e5a51b231ad1b6f3cdfd874ca27362a2a6a8
  Changeset pointed to by the tag bookmark tags/first_tag
  032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044
  $ mononoke_admin bookmarks -R repo get heads/another_branch
  032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044

# Re-import
  $ cd "$TESTTMP"
  $ with_stripped_logs gitimport "$GIT_REPO" --generate-bookmarks --cleanup-mononoke-bookmarks full-repo
  using repo "repo" repoid RepositoryId(0)
  GitRepo:$TESTTMP/repo-git 1 of 1 commit(s) already exist
  Ref: "refs/heads/master_bookmark": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Ref: "refs/remotes/origin/a_ref_prefixed_by_remotes_origin": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Ref: "refs/tags/changing_tag": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Ref: "refs/tags/recursive_tag": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Initializing repo: repo
  Initialized repo: repo
  All repos initialized. It took: 0 seconds
  Bookmark: "heads/master_bookmark": ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)) (already up-to-date)
  Bookmark: "remotes/origin/a_ref_prefixed_by_remotes_origin": ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)) (already up-to-date)
  Bookmark: "tags/changing_tag": ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)) (already up-to-date)
  Bookmark: "tags/recursive_tag": ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)) (already up-to-date)
  Bookmark: "heads/another_branch": ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)) (deleted)
  Bookmark: "tags/first_tag": ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)) (deleted)

# The tag and ref are no longer in Mononoke
  $ mononoke_admin bookmarks -R repo get tags/first_tag
  (not set)
  $ mononoke_admin bookmarks -R repo get heads/another_branch
  (not set)
