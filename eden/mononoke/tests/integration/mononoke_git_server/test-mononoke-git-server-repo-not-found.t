# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Test that the git server returns proper HTTP status codes when accessing
# repositories that don't exist or are not loaded on the current shard.
#
# - 404: Repository does not exist in the configuration
# - 503: Repository exists in configuration but is not loaded on this shard

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
  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO" --derive-hg --generate-bookmarks full-repo

# Setup a second repo in the config that we won't load
# Use a different REPOID to avoid "repoid used more than once" error
  $ cd "$TESTTMP/mononoke-config"
  $ REPOID=1 setup_mononoke_repo_config "unloaded_repo"

# Start up the Mononoke Git Service with --filter-repos to only load "repo"
# This simulates a sharded deployment where unloaded_repo is configured but
# not assigned to this shard
  $ mononoke_git_service --filter-repos "^repo$"

# Test 1: Accessing a completely nonexistent repo should return 404
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/nonexistent.git
  Cloning into 'nonexistent'...
  remote: Repository does not exist: nonexistent
  fatal: repository 'https://localhost:$LOCAL_PORT/repos/git/ro/nonexistent.git/' not found (glob)
  [128]

# Test 2: Accessing a repo that exists in config but is not loaded should return 503
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/unloaded_repo.git
  Cloning into 'unloaded_repo'...
  remote: Repository not available on this server: unloaded_repo
  fatal: unable to access 'https://*/repos/git/ro/unloaded_repo.git/': The requested URL returned error: 503 (glob)
  [128]

# Test 3: Verify that accessing the loaded repo still works
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
  Cloning into 'repo'...
  remote: Client correlator: * (glob)
  remote: Converting HAVE Git commits to Bonsais        
  remote: Converting WANT Git commits to Bonsais        
  remote: Collecting Bonsai commits to send to client        
  remote: Counting number of objects to be sent in packfile        
  remote: Generating trees and blobs stream        
  remote: Generating commits stream        
  remote: Generating tags stream        
  remote: Sending packfile stream        
