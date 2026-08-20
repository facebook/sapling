# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO="${TESTTMP}/repo-git"

# Setup Mononoke config with preloaded_cgdm_blobstore_key
  $ cd "$TESTTMP/mononoke-config"
  $ cat >> repos/repo/server.toml <<EOF
  > [git_configs]
  > preloaded_cgdm_blobstore_key = "test_cgdm_key"
  > EOF
  $ cd "$TESTTMP"

# Start up the Mononoke Git Service
  $ mononoke_git_service

# Set Mononoke as the Source of Truth
  $ set_mononoke_as_source_of_truth_for_git

# Clone the empty repo from Mononoke
  $ quiet git_client clone -q $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git "$GIT_REPO"
  $ cd "$GIT_REPO"
  $ git init -q

# Helper to create sequential commits by appending to 'file'
  $ mk_commits() {
  >   local start=$1 end=$2
  >   for i in $(seq $start $end); do
  >     echo "change_$i" >> file
  >     git commit -qam "Commit $i"
  >   done
  > }

# Phase 1: Push 15 commits (1-15). Use large file content so deltas are smaller
# than full objects, triggering delta encoding.
  $ seq 1 1000 > file
  $ git add file
  $ git commit -qam "Commit 1"
  $ quiet mk_commits 2 15

# Capture all the known Git objects from the repo
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/object_list

# Push all commits to Mononoke
  $ git_client push origin --all
  To https://*/repos/git/ro/repo.git (glob)
   * [new branch]      master_bookmark -> master_bookmark

# Wait for the bookmark to be created
  $ wait_for_git_bookmark_create refs/heads/master_bookmark

# Stop the server, run CGDM updater, then restart
  $ killandwait $MONONOKE_GIT_SERVICE_PID

# Run CGDM updater to build the initial mapping
# Component 0 has 15 changesets (< 20 max), so 0 new full components
  $ mononoke_admin --blobstore-put-behaviour Overwrite git-cgdm-updater --component-max-count 20 --component-max-size 10485760 repo -R repo --all-bookmarks --blobstore-key test_cgdm_key
  [repo] Loading existing CGDMComponents from mutable blobstore key 'test_cgdm_key'
  [repo] Mutable blobstore lookup completed in * (miss) (glob)
  [repo] Loading existing CGDMComponents from immutable blobstore key 'test_cgdm_key'
  [repo] Immutable blobstore lookup completed in * (miss) (glob)
  [repo] Loaded 0 existing components with 0 changesets (rebuild: false)
  [repo] Found 15 new changesets to process
  [repo] Finished calculating GDM sizes
  [repo] Finished assigning changesets to 1 components
  [repo] Storing CGDM blobs for 0 new full components
  [repo] Saving updated CGDMComponents to blobstore key 'test_cgdm_key'
  [repo] CGDM update complete

# Start up the Mononoke Git Service
  $ mononoke_git_service
# Clone the Git repo from Mononoke into clone1
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git clone1
  Cloning into 'clone1'...
  remote: Client correlator: * (glob)
  remote: Converting HAVE Git commits to Bonsais        
  remote: Converting WANT Git commits to Bonsais        
  remote: Collecting Bonsai commits to send to client        
  remote: Counting number of objects to be sent in packfile        
  remote: Generating trees and blobs stream        
  remote: Generating commits stream        
  remote: Generating tags stream        
  remote: Sending packfile stream        
# Verify that we get the same Git repo back that we started with
  $ cd clone1
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list
  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list

# Phase 2: Push 10 more commits (16-25), update CGDM, verify clone
  $ cd "$GIT_REPO"

# Update origin remote to point to the new server (restarted on a different port)
  $ git remote set-url origin $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git

  $ current_head=$(git rev-parse HEAD)
  $ quiet mk_commits 16 25

# Capture updated object list from our local repo
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/object_list

# Push the new commits to Mononoke
  $ git_client push -q origin master_bookmark

# Wait for the warm bookmark cache to catch up with the latest changes
  $ wait_for_git_bookmark_move HEAD $current_head

# Stop the server, run CGDM updater, then restart
  $ killandwait $MONONOKE_GIT_SERVICE_PID

# Run CGDM updater again
# Overwrite: loads phase 1 state (1 component, 15 changesets). Finds 10 new (16-25).
# Component 0 fills to 20 (full), overflow 5 go to component 1. 1 new full component.
  $ mononoke_admin --blobstore-put-behaviour Overwrite git-cgdm-updater --component-max-count 20 --component-max-size 10485760 repo -R repo --all-bookmarks --blobstore-key test_cgdm_key
  [repo] Loading existing CGDMComponents from mutable blobstore key 'test_cgdm_key'
  [repo] Mutable blobstore lookup completed in * (hit) (glob)
  [repo] Loaded 1 existing components with 15 changesets (rebuild: false)
  [repo] Found 10 new changesets to process
  [repo] Finished calculating GDM sizes
  [repo] Finished assigning changesets to 2 components
  [repo] Storing CGDM blobs for 1 new full components
  [repo] Saving updated CGDMComponents to blobstore key 'test_cgdm_key'
  [repo] CGDM update complete

# Start up the Mononoke Git Service
  $ mononoke_git_service
# Clone the Git repo from Mononoke into clone2
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git clone2
  Cloning into 'clone2'...
  remote: Client correlator: * (glob)
  remote: Converting HAVE Git commits to Bonsais        
  remote: Converting WANT Git commits to Bonsais        
  remote: Collecting Bonsai commits to send to client        
  remote: Counting number of objects to be sent in packfile        
  remote: Generating trees and blobs stream        
  remote: Generating commits stream        
  remote: Generating tags stream        
  remote: Sending packfile stream        
# Verify that we get the same Git repo back that we started with
  $ cd clone2
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list
  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list

# Phase 3: Push 10 more commits (26-35), update CGDM, verify clone
  $ cd "$GIT_REPO"

# Update origin remote to point to the new server (restarted on a different port)
  $ git remote set-url origin $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git

  $ current_head=$(git rev-parse HEAD)
  $ quiet mk_commits 26 35

# Capture updated object list from our local repo
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/object_list

# Push the new commits to Mononoke
  $ git_client push -q origin master_bookmark

# Wait for the warm bookmark cache to catch up with the latest changes
  $ wait_for_git_bookmark_move HEAD $current_head

# Stop the server, run CGDM updater, then restart
  $ killandwait $MONONOKE_GIT_SERVICE_PID

# Run CGDM updater again
# Overwrite: loads current state (2 components, 25 changesets). Finds 10 new (26-35).
# Component 1 goes from 5 to 15 (not full). 0 new full components.
  $ mononoke_admin --blobstore-put-behaviour Overwrite git-cgdm-updater --component-max-count 20 --component-max-size 10485760 repo -R repo --all-bookmarks --blobstore-key test_cgdm_key
  [repo] Loading existing CGDMComponents from mutable blobstore key 'test_cgdm_key'
  [repo] Mutable blobstore lookup completed in * (hit) (glob)
  [repo] Loaded 2 existing components with 25 changesets (rebuild: false)
  [repo] Found 10 new changesets to process
  [repo] Finished calculating GDM sizes
  [repo] Finished assigning changesets to 2 components
  [repo] Storing CGDM blobs for 0 new full components
  [repo] Saving updated CGDMComponents to blobstore key 'test_cgdm_key'
  [repo] CGDM update complete

# Start up the Mononoke Git Service
  $ mononoke_git_service
# Clone the Git repo from Mononoke into clone3
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git clone3
  Cloning into 'clone3'...
  remote: Client correlator: * (glob)
  remote: Converting HAVE Git commits to Bonsais        
  remote: Converting WANT Git commits to Bonsais        
  remote: Collecting Bonsai commits to send to client        
  remote: Counting number of objects to be sent in packfile        
  remote: Generating trees and blobs stream        
  remote: Generating commits stream        
  remote: Generating tags stream        
  remote: Sending packfile stream        
# Verify that we get the same Git repo back that we started with
  $ cd clone3
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list
  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list
