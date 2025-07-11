# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ HG_REPO="${TESTTMP}/repo"
  $ setup_common_config blob_files

# Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"

# Create a standalone nested tree for a ref to point to
  $ mkdir dir1
  $ echo "this is dir1/file1" > dir1/file1
  $ mkdir dir2
  $ echo "this is dir2/file2" > dir2/file2
  $ mkdir -p dir2/dir3/dir4/dir5/dir6/dir7
  $ echo "this is a deep nested file" > dir2/dir3/dir4/dir5/dir6/dir7/nested_file
  $ git add .
  $ git commit -qam "Added files and directories"

# Capture the root tree hash and nested blob hash
  $ root_tree_hash=$(git rev-parse HEAD^{tree})
  $ nested_blob_hash=$(git rev-parse HEAD:dir2/dir3/dir4/dir5/dir6/dir7/nested_file)

# Create a standalone blob for a ref to point to
  $ echo "I am a blob, all alone :(" > alone_blob
  $ git add .
  $ git commit -qam "Commit with alone blob"

# Capture the standalone blob hash
  $ blob_hash=$(git rev-parse HEAD:alone_blob)

# Move the master bookmark back two commits so that the refs to tree and blob are not covered by it
  $ git reset --hard HEAD~2
  HEAD is now at 8ce3eae Add file1

# Create an annotated tag pointing to the root tree of the repo
  $ git tag -a tag_to_tree $root_tree_hash -m "Tag pointing to root tree"
# Create a branch pointing to the root tree of the repo
  $ echo $root_tree_hash > .git/refs/heads/branch_to_root_tree
# Create a simple tag pointing to the root tree of the repo
  $ git tag simple_tag_to_tree $root_tree_hash
# Create a branch pointing to a blob in the repo
  $ echo $blob_hash > .git/refs/heads/branch_to_blob
# Create a recursive tag to check if it gets imported
  $ git config advice.nestedTag false
  $ git tag -a recursive_tag -m "this recursive tag points to tag_to_tree" $(git rev-parse tag_to_tree)
  $ cd "$TESTTMP"
  $ git clone --mirror "$GIT_REPO_ORIGIN" repo-git
  Cloning into bare repository 'repo-git'...
  done.

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ gitimport "$GIT_REPO" --concurrency 100 --generate-bookmarks --allow-content-refs full-repo
  [INFO] using repo "repo" repoid RepositoryId(0)
  [INFO] GitRepo:$TESTTMP/repo-git commit 1 of 1 - Oid:8ce3eae4 => Bid:032cd4dc
  [INFO] Ref: "refs/heads/branch_to_blob": None
  [INFO] Ref: "refs/heads/branch_to_root_tree": None
  [INFO] Ref: "refs/heads/master_bookmark": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  [INFO] Ref: "refs/tags/recursive_tag": None
  [INFO] Ref: "refs/tags/simple_tag_to_tree": None
  [INFO] Ref: "refs/tags/tag_to_tree": None
  [INFO] Initializing repo: repo
  [INFO] Initialized repo: repo
  [INFO] All repos initialized. It took: * seconds (glob)
  [INFO] Bookmark: "heads/master_bookmark": ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)) (created)

# Verify if we have stored the root tree blob pointed to by heads/branch_to_root_tree, tags/tag_to_tree and tags/simple_tag_to_tree
  $ mononoke_admin git-objects -R repo fetch --id $root_tree_hash
  The object is a Git Tree
  
  TreeRef {
      entries: [
          EntryRef {
              mode: EntryMode(0o40000),
              filename: "dir1",
              oid: Sha1(1688a24aee0ac76cbb13bd72967339c13deae505),
          },
          EntryRef {
              mode: EntryMode(0o40000),
              filename: "dir2",
              oid: Sha1(5146666596d2520dfd1d3c2acdc4b1448745a349),
          },
          EntryRef {
              mode: EntryMode(0o100644),
              filename: "file1",
              oid: Sha1(433eb172726bc7b6d60e8d68efb0f0ef4e67a667),
          },
      ],
  }


# Verify if we have stored the standalone blob pointed to by heads/branch_to_blob
  $ mononoke_admin git-objects -R repo fetch --id $blob_hash
  The object is a Git Blob
  
  "I am a blob, all alone :(\n"


# Now validate if we have stored the nested file within the root tree blob
  $ mononoke_admin git-objects -R repo fetch --id $nested_blob_hash
  The object is a Git Blob
  
  "this is a deep nested file\n"

