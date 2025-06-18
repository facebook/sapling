# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"

# Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ old_head=$(git rev-parse HEAD)
  $ git tag -a -m "new tag" first_tag
  $ echo "this is file2" > file2
  $ git add file2
  $ git commit -qam "Add file2"
  $ git tag -a empty_tag -m ""
  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.
  $ cd $GIT_REPO
  $ current_head=$(git rev-parse HEAD)

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO" --derive-hg --generate-bookmarks full-repo

# Set Mononoke as the Source of Truth
  $ set_mononoke_as_source_of_truth_for_git

# Start up the Mononoke Git Service
  $ mononoke_git_service
# Clone the Git repo from Mononoke
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git

# Create a standalone nested tree for a ref to point to
  $ cd repo
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
  HEAD is now at e8615d6 Add file2
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


# Create a sample commit to push the repo forward
  $ echo "just a file bruh" > just_a_file
  $ git add .
  $ git commit -qam "just a commit"


# Push all the changes made so far
  $ git_client push origin master_bookmark branch_to_blob branch_to_root_tree tag_to_tree simple_tag_to_tree recursive_tag
  To https://*/repos/git/ro/repo.git (glob)
     e8615d6..dbe48b4  master_bookmark -> master_bookmark
   * [new branch]      branch_to_blob -> branch_to_blob
   * [new branch]      branch_to_root_tree -> branch_to_root_tree
   * [new tag]         tag_to_tree -> tag_to_tree
   * [new tag]         simple_tag_to_tree -> simple_tag_to_tree
   * [new tag]         recursive_tag -> recursive_tag

# Wait for the WBC to catch up
  $ wait_for_git_bookmark_move HEAD $current_head

# Since Mononoke Git clone doesn't work with standalone tree refs, verify if we have stored the root tree
# blob pointed to by heads/branch_to_root_tree, tags/tag_to_tree and tags/simple_tag_to_tree
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
          EntryRef {
              mode: EntryMode(0o100644),
              filename: "file2",
              oid: Sha1(f138820097c8ef62a012205db0b1701df516f6d5),
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
