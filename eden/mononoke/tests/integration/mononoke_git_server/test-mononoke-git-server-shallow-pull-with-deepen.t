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
# Visualize the graph to verify its the right shape
  $ git log --all --decorate --oneline --graph
  *   18a6f40 (R2) Merge branch 'R3' into R2
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
  | * 8ca6d2a (HEAD -> master_bookmark) Adding fileJ
  | * 7eda99f Adding fileI
  | * 6e43a74 Adding fileH
  | * 99f1ee9 Adding fileG
  |/  
  * 83ef99f initial commit

# Import the repo into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO_ORIGIN" --derive-hg --generate-bookmarks full-repo

# Start up the Mononoke Git Service
  $ mononoke_git_service

# Clone the repo using Mononoke with depth = 2
  $ quiet git_client clone --depth=2 --no-single-branch $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
  $ cd $REPONAME
# Visualize the graph to verify its the right shape
  $ git log --all --decorate --oneline --graph
  * 8ca6d2a (HEAD -> master_bookmark, origin/master_bookmark, origin/HEAD) Adding fileJ
  * 7eda99f (grafted) Adding fileI
  * 47156f5 (origin/R1) Adding fileA and fileB
  * 83ef99f (grafted) initial commit
  *   18a6f40 (origin/R2) Merge branch 'R3' into R2
  |\  
  | * 619f44e (origin/R3) Adding fileE
  | * a9ff5f9 (grafted) Adding fileC and fileD
  * 9089a8c (grafted) Adding fileF

# Clone the repo using Vanilla Git with depth = 2
  $ cd $TESTTMP
  $ git clone --depth=2 --no-single-branch file://"$GIT_REPO_ORIGIN" vanilla-repo
  Cloning into 'vanilla-repo'...
  $ cd vanilla-repo
# Visualize the graph to verify its the right shape
  $ git log --all --decorate --oneline --graph  
  * 8ca6d2a (HEAD -> master_bookmark, origin/master_bookmark, origin/HEAD) Adding fileJ
  * 7eda99f (grafted) Adding fileI
  * 47156f5 (origin/R1) Adding fileA and fileB
  * 83ef99f (grafted) initial commit
  *   18a6f40 (origin/R2) Merge branch 'R3' into R2
  |\  
  | * 619f44e (origin/R3) Adding fileE
  | * a9ff5f9 (grafted) Adding fileC and fileD
  * 9089a8c (grafted) Adding fileF

# Pull from Vanilla Git with deepen = 2, which is shallow with depth from the client side instead of server side
  $ cd $TESTTMP/vanilla-repo
  $ quiet git_client pull --deepen=2
# Visualize the graph to verify its the right shape
  $ git log --all --decorate --oneline --graph
  * 8ca6d2a (HEAD -> master_bookmark, origin/master_bookmark, origin/HEAD) Adding fileJ
  * 7eda99f Adding fileI
  * 6e43a74 Adding fileH
  * 99f1ee9 (grafted) Adding fileG
  *   18a6f40 (origin/R2) Merge branch 'R3' into R2
  |\  
  | * 619f44e (origin/R3) Adding fileE
  * | 9089a8c Adding fileF
  * |   12a34ee Merge branch 'R1' into R2
  |\ \  
  | |/  
  |/|   
  | * 47156f5 (grafted, origin/R1) Adding fileA and fileB
  * a9ff5f9 Adding fileC and fileD
  * 83ef99f initial commit
# Pull from Mononoke with deepen = 2, which is shallow with depth from the client side instead of server side
  $ cd $TESTTMP/$REPONAME
  $ quiet git_client pull --deepen=2
# Visualize the graph to verify its the right shape
  $ git log --all --decorate --oneline --graph
  * 8ca6d2a (HEAD -> master_bookmark, origin/master_bookmark, origin/HEAD) Adding fileJ
  * 7eda99f Adding fileI
  * 6e43a74 Adding fileH
  * 99f1ee9 (grafted) Adding fileG
  *   18a6f40 (origin/R2) Merge branch 'R3' into R2
  |\  
  | * 619f44e (origin/R3) Adding fileE
  * | 9089a8c Adding fileF
  * |   12a34ee Merge branch 'R1' into R2
  |\ \  
  | |/  
  |/|   
  | * 47156f5 (grafted, origin/R1) Adding fileA and fileB
  * a9ff5f9 Adding fileC and fileD
  * 83ef99f initial commit
