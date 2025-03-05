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
  $ SCUBA="$TESTTMP/scuba.json"

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
  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.

# Capture all the known Git objects from the repo
  $ cd $GIT_REPO
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/object_list

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO" --derive-hg --generate-bookmarks full-repo

# Before cloning from Mononoke, let's delete some required object
  $ cd $TESTTMP/blobstore/blobs
  $ find . -type f -regex ".*git_packfile_base_item.*" -delete
  $ find . -type f -regex ".*git_object.*" -delete
  $ cd "$TESTTMP"

# Start up the Mononoke Git Service
  $ mononoke_git_service
# Clone the Git repo from Mononoke. This should fail
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
  remote: Failure in fetching Packfile Item from stream
  
  Caused by:
      0: Error in fetching raw git object bytes for object Sha1(fb02ed046a1e75fe2abb8763f7c715496ae36353) while fetching-and-storing packfile item
      1: The object corresponding to object ID fb02ed046a1e75fe2abb8763f7c715496ae36353 or its packfile item does not exist in the data store
  fatal: early EOF
  fatal: fetch-pack: invalid index-pack output
  [128]

# Verify that the packfile error shows up in scuba logs
  $ jq .normal "$SCUBA" | grep -e "packfile_read_error" | sort
    "packfile_read_error": "Failure in fetching Packfile Item from stream\n\nCaused by:\n    0: Error in fetching raw git object bytes for object Sha1(fb02ed046a1e75fe2abb8763f7c715496ae36353) while fetching-and-storing packfile item\n    1: The object corresponding to object ID fb02ed046a1e75fe2abb8763f7c715496ae36353 or its packfile item does not exist in the data store",
