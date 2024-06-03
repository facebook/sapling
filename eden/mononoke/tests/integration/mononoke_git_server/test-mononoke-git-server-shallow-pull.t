# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ ENABLED_DERIVED_DATA='["git_commits", "git_trees", "git_delta_manifests", "unodes", "filenodes", "hgchangesets"]' setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ HG_REPO="${TESTTMP}/repo-hg"
  $ BUNDLE_PATH="${TESTTMP}/repo_bundle.bundle"
  $ cat >> repos/repo/server.toml <<EOF
  > [source_control_service]
  > permit_writes = true
  > EOF

# Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
# Create dummy commit on master
  $ git commit -q --allow-empty -m "initial commit" 
# Create branches R1 and R2
  $ git branch R1
  $ git branch R2
# Checkout R1 and create a commit on it
  $ git checkout -q R1
  $ echo "File A.File A.File A.File A.File A.File A.File A.File A.File A.File A.File A.File A.File A.File A.File A.File A." > fileA
  $ echo "File B.File B.File B.File B.File B.File B.File B.File B.File B.File B.File B.File B.File B.File B.File B.File B." > fileB
  $ git add .
  $ git commit -qam "Adding fileA and fileB"
# Checkout R2 and create a commit on it
  $ git checkout -q R2
  $ echo "File C.File C.File C.File C.File C.File C.File C.File C.File C.File C.File C.File C.File C.File C.File C.File C." > fileC
  $ echo "File D.File D.File D.File D.File D.File D.File D.File D.File D.File D.File D.File D.File D.File D.File D.File D." > fileD
  $ git add .
  $ git commit -qam "Adding fileC and fileD"
  $ prev_head=$(git rev-parse HEAD)

# Visualize the graph to verify its the right shape
  $ git log --all --decorate --oneline --graph
  * 47156f5 (R1) Adding fileA and fileB
  | * a9ff5f9 (HEAD -> R2) Adding fileC and fileD
  |/  
  * 83ef99f (master) initial commit

# Clone the repo with the changes made so far
  $ cd "$TESTTMP"
  $ git clone file://"$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...

# Import the repo into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO_ORIGIN" --derive-hg --generate-bookmarks full-repo

# Start up the Mononoke Git Service
  $ mononoke_git_service

# Clone the repo using Mononoke
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
  Cloning into 'repo'...

# Add more changes to the origin repo
  $ cd "$GIT_REPO_ORIGIN"
# Create a new branch R3 from R2
  $ git checkout -qb R3
  $ echo "File E.File E.File E.File E.File E.File E.File E.File E.File E.File E.File E.File E.File E.File E.File E.File E." > fileE
  $ git add .
  $ git commit -qam "Adding fileE" 
# Merge commits from R1 into R2
  $ git checkout -q R2
  $ git merge R1 -q
# Create another commit on R2
  $ echo "File F.File F.File F.File F.File F.File F.File F.File F.File F.File F.File F.File F.File F.File F.File F.File F." > fileF
  $ git add .
  $ git commit -qam "Adding fileF"
# Merge commits from R3 into R2
  $ git merge R3 -q
# Visualize the graph to verify its the right shape
  $ git log --all --decorate --oneline --graph
  *   18a6f40 (HEAD -> R2) Merge branch 'R3' into R2
  |\  
  | * 619f44e (R3) Adding fileE
  * | 9089a8c Adding fileF
  * |   12a34ee Merge branch 'R1' into R2
  |\ \  
  | |/  
  |/|   
  | * 47156f5 (R1) Adding fileA and fileB
  * | a9ff5f9 Adding fileC and fileD
  |/  
  * 83ef99f (master) initial commit

# Perform a shallow pull of the repo with depth = 2 and list the commits. Commit 12a34ee should NOT be present
# since it exists at depth=3 and the client did not already have it after the clone
  $ cd $GIT_REPO
  $ quiet git pull --depth=2
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | grep commit | sort
  18a6f40de35ce474e240faa7298ae2b5979751c8 commit 
  47156f5aa75771131c092593377d7e74d0c38baa commit 
  619f44e4b1883ec6cafa608967d2f314f2224792 commit 
  83ef99fe983e803ce5365adb6e3be59043bd7aad commit 
  9089a8c5d6429a5dfa430d1abefd73234894c4df commit 
  a9ff5f932c4a81f710d754b02e20dcbb8236cc23 commit 
# Capture the list of objects in the repo
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/object_list 

# Pull the latest changes from Mononoke and verify we get the same end state
  $ cd $GIT_REPO
# Wait for the warm bookmark cache to catch up with the latest changes
  $ wait_for_git_bookmark_move HEAD $prev_head
  $ quiet git_client pull --depth=2
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | grep commit | sort
  18a6f40de35ce474e240faa7298ae2b5979751c8 commit 
  47156f5aa75771131c092593377d7e74d0c38baa commit 
  619f44e4b1883ec6cafa608967d2f314f2224792 commit 
  83ef99fe983e803ce5365adb6e3be59043bd7aad commit 
  9089a8c5d6429a5dfa430d1abefd73234894c4df commit 
  a9ff5f932c4a81f710d754b02e20dcbb8236cc23 commit 
# Capture the list of objects in the repo
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list
# Validate that the objects match
  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list
