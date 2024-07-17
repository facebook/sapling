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

# Perform a shallow clone of the repo with depth = 1 and list the commits
  $ cd "$TESTTMP"
  $ git clone --depth=1 file://"$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  $ cd $GIT_REPO
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | grep commit | sort
  18a6f40de35ce474e240faa7298ae2b5979751c8 commit 
# Even though the clone returns a single commit, validate that it contains the entire working copy
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/object_list

  $ cd "$TESTTMP"
  $ rm -rf $GIT_REPO

# Perform a shallow clone of the repo with depth = 3 and list the commits
  $ git clone --depth=3 file://"$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  $ cd $GIT_REPO
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | grep commit | sort
  12a34ee8026e5118cf6a2123c94057d1c8f9c5bb commit 
  18a6f40de35ce474e240faa7298ae2b5979751c8 commit 
  619f44e4b1883ec6cafa608967d2f314f2224792 commit 
  9089a8c5d6429a5dfa430d1abefd73234894c4df commit 
  a9ff5f932c4a81f710d754b02e20dcbb8236cc23 commit 
  $ cd "$TESTTMP"
  $ rm -rf $GIT_REPO

# Attempt to perform a shallow clone with depth = 0. This result result in error
  $ git clone --depth=0 file://"$GIT_REPO_ORIGIN"
  fatal: depth 0 is not a positive number
  [128]

# Import the repo into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO_ORIGIN" --derive-hg --generate-bookmarks full-repo

# Start up the Mononoke Git Service
  $ mononoke_git_service

# Perform Mononoke clone with the depth of 3 and it should have the expected output
  $ cd "$TESTTMP"
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --depth=3
  Cloning into 'repo'...
  $ cd $REPONAME
# Validate that the list of commits returned match the expected output
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | grep commit | sort
  12a34ee8026e5118cf6a2123c94057d1c8f9c5bb commit 
  18a6f40de35ce474e240faa7298ae2b5979751c8 commit 
  619f44e4b1883ec6cafa608967d2f314f2224792 commit 
  9089a8c5d6429a5dfa430d1abefd73234894c4df commit 
  a9ff5f932c4a81f710d754b02e20dcbb8236cc23 commit 
  $ cd "$TESTTMP"
  $ rm -rf $REPONAME

# Perform Mononoke clone with the depth of 1
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --depth=1
  Cloning into 'repo'...
  $ cd $REPONAME
# Validate that the list of commits returned match the expected output
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | grep commit | sort
  18a6f40de35ce474e240faa7298ae2b5979751c8 commit 
# Even though the clone returns a single commit, validate that it contains the entire working copy
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list
  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list
