# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Test that the git server can dynamically load a repository when it exists
# in config but is not loaded on the current shard.
#
# This test sets up two repositories in the configuration, but starts the
# server with only one loaded. When a request is made for the second repository,
# the server should dynamically load it (when the JK is enabled) instead of
# returning a 503 error.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"

# Setup git repository for the first repo
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.

# Import the first repo into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO" --derive-hg --generate-bookmarks full-repo

# Setup a second repo in the configuration
  $ SECOND_GIT_REPO_ORIGIN="${TESTTMP}/origin/second-repo-git"
  $ SECOND_GIT_REPO="${TESTTMP}/second-repo-git"
  $ mkdir -p "$SECOND_GIT_REPO_ORIGIN"
  $ cd "$SECOND_GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is second repo file1" > file1
  $ git add file1
  $ git commit -qam "Add file1 in second repo"
  $ cd "$TESTTMP"
  $ git clone "$SECOND_GIT_REPO_ORIGIN"
  Cloning into 'second-repo-git'...
  done.

# Configure the second repo in Mononoke with a different REPOID
  $ cd "$TESTTMP/mononoke-config"
  $ REPOID=1 setup_mononoke_repo_config "second_repo"

# Import the second repo into Mononoke
  $ cd "$TESTTMP"
  $ REPOID=1 REPONAME="second_repo" quiet gitimport "$SECOND_GIT_REPO" --derive-hg --generate-bookmarks full-repo

# Enable the JustKnob for dynamic repo loading
  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:git_server_dynamic_repo_loading": true
  >   }
  > }
  > EOF

# Start up the Mononoke Git Service with --filter-repos to only load "repo"
# This simulates a sharded deployment where second_repo is configured but
# not initially loaded on this shard
  $ mononoke_git_service --filter-repos "^repo$"

# Test 1: Verify that the first repo (already loaded) works normally
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git first_repo_clone
  Cloning into 'first_repo_clone'...
  remote: Client correlator: * (glob)
  remote: Converting HAVE Git commits to Bonsais        
  remote: Converting WANT Git commits to Bonsais        
  remote: Collecting Bonsai commits to send to client        
  remote: Counting number of objects to be sent in packfile        
  remote: Generating trees and blobs stream        
  remote: Generating commits stream        
  remote: Generating tags stream        
  remote: Sending packfile stream        

# Test 2: Verify that accessing the second repo (not initially loaded) succeeds
# because the server dynamically loads it
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/second_repo.git second_repo_clone
  Cloning into 'second_repo_clone'...
  remote: Client correlator: * (glob)
  remote: Converting HAVE Git commits to Bonsais        
  remote: Converting WANT Git commits to Bonsais        
  remote: Collecting Bonsai commits to send to client        
  remote: Counting number of objects to be sent in packfile        
  remote: Generating trees and blobs stream        
  remote: Generating commits stream        
  remote: Generating tags stream        
  remote: Sending packfile stream        

# Verify that the cloned second repo has the expected content
  $ cd second_repo_clone
  $ cat file1
  this is second repo file1
