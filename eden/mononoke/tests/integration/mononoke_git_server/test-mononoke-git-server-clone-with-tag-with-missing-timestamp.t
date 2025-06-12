# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"

# Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ git tag -a -m"new tag" first_tag
  $ echo "this is file2" > file2
  $ git add file2
  $ git commit -qam "Add file2"
  $ git tag -a empty_tag -m ""
Checkout to the previous commit
  $ git checkout HEAD~1 -q
Create commit in detached state so its not tracked by any branch
  $ echo "this is file3" > file3
  $ git add file3
  $ git commit -qam "Add file3"

# Create a correct tag
  $ git tag -a correct_tag -m ""
# Now create an incorrect tag from this correct tag
  $ git show-ref correct_tag
  46966e726615628dd6f977eeb78a3cb4b0abe6c2 refs/tags/correct_tag
  $ git cat-file -p 46966e726615628dd6f977eeb78a3cb4b0abe6c2
  object d9dc1768c477b85bd1d8bd2d238f234cfe8fbdc4
  type commit
  tag correct_tag
  tagger mononoke <mononoke@mononoke> 946684800 +0000
  

# First, show why the tag is invalid, and why git tries to prevent us from creating it
  $ git cat-file -p 46966e726615628dd6f977eeb78a3cb4b0abe6c2 | head -c 111 | { printf "%s\n" "$(cat)"; } | git mktag
  error: tag input does not pass fsck: missingSpaceBeforeDate: invalid author/committer line - missing space before date
  fatal: tag on stdin did not pass our strict fsck check
  [128]
# Nevermind: just need to ask nicely
  $ git cat-file -p 46966e726615628dd6f977eeb78a3cb4b0abe6c2 | head -c 111 | { printf "%s\n" "$(cat)"; } | git hash-object -w --stdin -t tag --literally
  2e1bada5af3034c3daa2835ddbec6dfd10cdfe17
# Show our malformed tag for info
  $ git cat-file -p 2e1bada5af3034c3daa2835ddbec6dfd10cdfe17
  object d9dc1768c477b85bd1d8bd2d238f234cfe8fbdc4
  type commit
  tag correct_tag
  tagger mononoke <mononoke@mononoke>
  $ git cat-file -t 2e1bada5af3034c3daa2835ddbec6dfd10cdfe17
  tag
# Make a ref that points to this incorrect tag
  $ echo 2e1bada5af3034c3daa2835ddbec6dfd10cdfe17 > .git/refs/tags/incorrect_tag

Go back to the master_bookmark branch
  $ git checkout master_bookmark -q

# Capture all the known Git objects from the repo
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/object_list

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO_ORIGIN" --derive-hg --generate-bookmarks full-repo

# Start up the Mononoke Git Service
  $ mononoke_git_service
# Clone the Git repo from Mononoke
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
  Cloning into 'repo'...
  remote: Converting HAVE Git commits to Bonsais        
  remote: Converting WANT Git commits to Bonsais        
  remote: Collecting Bonsai commits to send to client        
  remote: Counting number of objects to be sent in packfile        
  remote: Generating trees and blobs stream        
  remote: Generating commits stream        
  remote: Generating tags stream        
  remote: Sending packfile stream        
  fatal: bad object 2e1bada5af3034c3daa2835ddbec6dfd10cdfe17
  fatal: remote did not send all necessary objects
  [128]

# Verify that we get the same Git repo back that we started with
#  $ cd $REPONAME
#  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list
#  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list  
