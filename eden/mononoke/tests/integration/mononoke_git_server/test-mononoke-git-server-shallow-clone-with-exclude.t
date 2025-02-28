# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
  $ . "${TEST_FIXTURES}/library.sh"
  $ export ENABLE_BOOKMARK_CACHE=1
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ MONONOKE_GIT_REPO="${TESTTMP}/repo-git"
  $ VANILLA_GIT_REPO="${TESTTMP}/vanilla-repo"
# Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
# Create dummy commit on master_bookmark
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
# Checkout master and add a few more commits to it
  $ git checkout -q master_bookmark
  $ echo "File G.File G.File G.File G.File G.File G.File G.File G.File G.File G.File G.File G.File G.File G.File G.File G." > fileG
  $ git add .
  $ git commit -qam "Adding fileG"
  $ echo "File H.File H.File H.File H.File H.File H.File H.File H.File H.File H.File H.File H.File H.File H.File H.File H." > fileH
  $ git add .
  $ git commit -qam "Adding fileH"
  $ echo "File I.File I.File I.File I.File I.File I.File I.File I.File I.File I.File I.File I.File I.File I.File I.File I." > fileI
  $ git add .
  $ git commit -qam "Adding fileI"
  $ echo "File J.File J.File J.File J.File J.File J.File J.File J.File J.File J.File J.File J.File J.File J.File J.File J." > fileJ
  $ git add .
  $ git commit -qam "Adding fileJ"
  $ git checkout -q R2

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
  | * 8ca6d2a (master_bookmark) Adding fileJ
  | * 7eda99f Adding fileI
  | * 6e43a74 Adding fileH
  | * 99f1ee9 Adding fileG
  |/  
  * 83ef99f initial commit
# Git's support for shallow-exclude seems to be broken. Based on documentation (https://fburl.com/ox4tabpi), the below should be the output of shallow-exclude
# clones for each of the respective branches. But that doesn't seem to be the case (P1741348601), so I am including the direct rev-list output to serve as comparision basis
# for Mononoke Git
  $ git rev-list heads/R2 --not heads/R3
  18a6f40de35ce474e240faa7298ae2b5979751c8
  9089a8c5d6429a5dfa430d1abefd73234894c4df
  12a34ee8026e5118cf6a2123c94057d1c8f9c5bb
  47156f5aa75771131c092593377d7e74d0c38baa
  $ git rev-list heads/R2 --not heads/R1
  18a6f40de35ce474e240faa7298ae2b5979751c8
  9089a8c5d6429a5dfa430d1abefd73234894c4df
  619f44e4b1883ec6cafa608967d2f314f2224792
  12a34ee8026e5118cf6a2123c94057d1c8f9c5bb
  a9ff5f932c4a81f710d754b02e20dcbb8236cc23
  $ git rev-list heads/R2 --not heads/master_bookmark
  18a6f40de35ce474e240faa7298ae2b5979751c8
  9089a8c5d6429a5dfa430d1abefd73234894c4df
  619f44e4b1883ec6cafa608967d2f314f2224792
  12a34ee8026e5118cf6a2123c94057d1c8f9c5bb
  a9ff5f932c4a81f710d754b02e20dcbb8236cc23
  47156f5aa75771131c092593377d7e74d0c38baa
# Import the repo into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO_ORIGIN" --derive-hg --generate-bookmarks full-repo
# Start up the Mononoke Git Service
  $ mononoke_git_service
# Clone the repo using Mononoke excluding commits reachable from heads/R3 and reachable from heads/R2
  $ quiet git_client clone --shallow-exclude heads/R3 $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git  
  $ cd repo
  $ git rev-list --all
  18a6f40de35ce474e240faa7298ae2b5979751c8
# Clone the repo using Mononoke excluding commits reachable from heads/R1
  $ cd "$TESTTMP"
  $ rm -rf repo
  $ quiet git_client clone --shallow-exclude heads/R1 $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
  $ cd repo
  $ git rev-list --all
  18a6f40de35ce474e240faa7298ae2b5979751c8
  9089a8c5d6429a5dfa430d1abefd73234894c4df
  619f44e4b1883ec6cafa608967d2f314f2224792
  12a34ee8026e5118cf6a2123c94057d1c8f9c5bb
  a9ff5f932c4a81f710d754b02e20dcbb8236cc23
# Clone the repo using Mononoke excluding commits reachable from heads/master_bookmark
  $ cd "$TESTTMP"
  $ rm -rf repo
  $ quiet git_client clone --shallow-exclude heads/master_bookmark $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
  $ cd repo
  $ git rev-list --all
  18a6f40de35ce474e240faa7298ae2b5979751c8
  9089a8c5d6429a5dfa430d1abefd73234894c4df
  619f44e4b1883ec6cafa608967d2f314f2224792
  12a34ee8026e5118cf6a2123c94057d1c8f9c5bb
  a9ff5f932c4a81f710d754b02e20dcbb8236cc23
  47156f5aa75771131c092593377d7e74d0c38baa
