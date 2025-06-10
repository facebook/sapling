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
  $ git tag -a -m "incorrectly named tag" incorrect_tag
  $ mv .git/refs/tags/incorrect_tag .git/refs/incorrect_tag
  $ echo "this is file2" > file2
  $ git add file2
  $ git commit -qam "Add file2"
  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.

# Capture all the known Git objects from the repo
  $ cd $GIT_REPO_ORIGIN
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/object_list
  $ git show-ref -d
  e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/heads/master_bookmark
  6f5d55eb96433995aca8f272263ae2ea50e40ec7 refs/incorrect_tag
  8ce3eae44760b500bf3f2c3922a95dcd3c908e9e refs/incorrect_tag^{}

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO_ORIGIN" --derive-hg --generate-bookmarks full-repo

# Start up the Mononoke Git Service
  $ mononoke_git_service
# Clone the Git repo from Mononoke
  $ git_client clone --mirror $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git repo
  Cloning into bare repository 'repo'...
  remote: Converting HAVE Git commits to Bonsais        
  remote: Converting WANT Git commits to Bonsais        
  remote: Collecting Bonsai commits to send to client        
  remote: Counting number of objects to be sent in packfile        
  remote: Generating trees and blobs stream        
  remote: Generating commits stream        
  remote: Generating tags stream        
  remote: Sending packfile stream        
# Verify that we get the same Git repo back that we started with
  $ cd $REPONAME  
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list
  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list

# List the set of refs known by the server. Even here the server doesn't return incorrect_tag as an annotated tag
  $ git_client ls-remote $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
  e8615d6f149b876be0a2f30a1c5bf0c42bf8e136	HEAD
  e8615d6f149b876be0a2f30a1c5bf0c42bf8e136	refs/heads/master_bookmark
  6f5d55eb96433995aca8f272263ae2ea50e40ec7	refs/incorrect_tag
  8ce3eae44760b500bf3f2c3922a95dcd3c908e9e	refs/incorrect_tag^{}
