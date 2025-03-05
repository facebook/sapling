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
  4fc57ab40b2792507fa920b2ffc730eedd9d8bce	refs/heads/branch_to_blob
  e6ebdbd95192b452cf0ff0535f22812630a8bc5e	refs/heads/branch_to_root_tree
  8ce3eae44760b500bf3f2c3922a95dcd3c908e9e	refs/heads/master_bookmark
  81d07e582d75e193b263d0d5778927339dcdce03	refs/tags/recursive_tag
  e6ebdbd95192b452cf0ff0535f22812630a8bc5e	refs/tags/recursive_tag^{}
  e6ebdbd95192b452cf0ff0535f22812630a8bc5e	refs/tags/simple_tag_to_tree
  b2c2c30fb8db9fd7bd004f324ba9554268641d7d	refs/tags/tag_to_tree
  e6ebdbd95192b452cf0ff0535f22812630a8bc5e	refs/tags/tag_to_tree^{}

# Clone the repo from Mononoke. Because we do not support refs to trees and blobs in Mononoke Git, the clone fails
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git  
  Cloning into 'repo'...
  remote: Converting HAVE Git commits to Bonsais        
  remote: Converting WANT Git commits to Bonsais        
  remote: Collecting Bonsai commits to send to client        
  remote: Counting number of objects to be sent in packfile        
  remote: Generating trees and blobs stream        
  remote: Generating commits stream        
  remote: Generating tags stream        
  remote: Sending packfile stream        
  fatal: did not receive expected object 5146666596d2520dfd1d3c2acdc4b1448745a349
  fatal: fetch-pack: invalid index-pack output
  [128]
