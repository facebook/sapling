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
  $ git tag -a -m "new tag" first_tag
# Capture the root tree hash
  $ root_tree_hash=$(git cat-file commit $(git rev-list --max-parents=0 HEAD) | grep '^tree' | awk '{print $2}')
# Capture the first blob hash
  $ first_blob_hash=$(git ls-tree $(git rev-list --max-parents=0 HEAD) | awk '{print $3}' | head -n 1)
# Create an annotated tag pointing to the root tree of the repo
  $ git tag -a tag_to_tree $root_tree_hash -m "Tag pointing to root tree"  
# Create a branch pointing to the root tree of the repo
  $ echo $root_tree_hash > .git/refs/heads/branch_to_root_tree
# Create a simple tag pointing to the root tree of the repo
  $ git tag simple_tag_to_tree $root_tree_hash
# Create a branch pointing to a blob in the repo
  $ echo $first_blob_hash > .git/refs/heads/branch_to_blob
# Create a recursive tag to check if it gets imported
  $ git config advice.nestedTag false
  $ git tag -a recursive_tag -m "this recursive tag points to tag_to_tree" $(git rev-parse tag_to_tree)
  $ cd "$TESTTMP"
  $ git clone --mirror "$GIT_REPO_ORIGIN" repo-git
  Cloning into bare repository 'repo-git'...
  done.

# Capture all the known Git objects from the repo
  $ cd $GIT_REPO
  $ git rev-list --objects --no-object-names --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/object_list

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO" --concurrency 100 --generate-bookmarks --allow-content-refs full-repo

# Start up the Mononoke Git Service
  $ mononoke_git_service
# List the known refs for the repo from Mononoke
  $ git_client ls-remote $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
  8ce3eae44760b500bf3f2c3922a95dcd3c908e9e	HEAD
  433eb172726bc7b6d60e8d68efb0f0ef4e67a667	refs/heads/branch_to_blob
  cb2ef838eb24e4667fee3a8b89c930234ae6e4bb	refs/heads/branch_to_root_tree
  8ce3eae44760b500bf3f2c3922a95dcd3c908e9e	refs/heads/master_bookmark
  8963e1f55d1346a07c3aec8c8fc72bf87d0452b1	refs/tags/first_tag
  8ce3eae44760b500bf3f2c3922a95dcd3c908e9e	refs/tags/first_tag^{}
  a8c14233f14d030ddbc16eb955df7fbc1922a5de	refs/tags/recursive_tag
  cb2ef838eb24e4667fee3a8b89c930234ae6e4bb	refs/tags/recursive_tag^{}
  cb2ef838eb24e4667fee3a8b89c930234ae6e4bb	refs/tags/simple_tag_to_tree
  98517855d851d4ed98d78cf903cefa46d95f3623	refs/tags/tag_to_tree
  cb2ef838eb24e4667fee3a8b89c930234ae6e4bb	refs/tags/tag_to_tree^{}

# Clone the repo from Mononoke. Because we do not support refs to trees and blobs in Mononoke Git, the clone fails
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git  

# Verify that we get the same Git repo back that we started with
  $ cd $REPONAME  
  $ git rev-list --objects --no-object-names --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list
  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list
