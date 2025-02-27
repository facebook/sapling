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
# Create few commits with different commit times
  $ echo "File A.File A.File A.File A.File A.File A.File A.File A.File A.File A.File A.File A.File A.File A.File A.File A." > fileA
  $ echo "File B.File B.File B.File B.File B.File B.File B.File B.File B.File B.File B.File B.File B.File B.File B.File B." > fileB
  $ git add .
  $ GIT_AUTHOR_DATE='05/05/0000 00:00 +0000' GIT_COMMITTER_DATE='01/01/0000 00:00 +0000' git commit -qam "Adding fileA and fileB"

  $ echo "File C.File C.File C.File C.File C.File C.File C.File C.File C.File C.File C.File C.File C.File C.File C.File C." > fileC
  $ echo "File D.File D.File D.File D.File D.File D.File D.File D.File D.File D.File D.File D.File D.File D.File D.File D." > fileD
  $ git add .
  $ GIT_AUTHOR_DATE='05/05/0000 00:00 +0000' GIT_COMMITTER_DATE='02/01/0000 00:00 +0000' git commit -qam "Adding fileC and fileD"

  $ echo "File E.File E.File E.File E.File E.File E.File E.File E.File E.File E.File E.File E.File E.File E.File E.File E." > fileE
  $ git add .
  $ GIT_AUTHOR_DATE='05/05/0000 00:00 +0000' GIT_COMMITTER_DATE='03/01/0000 00:00 +0000' git commit -qam "Adding fileE" 

  $ echo "File F.File F.File F.File F.File F.File F.File F.File F.File F.File F.File F.File F.File F.File F.File F.File F." > fileF
  $ git add .
  $ GIT_AUTHOR_DATE='05/05/0000 00:00 +0000' GIT_COMMITTER_DATE='04/01/0000 00:00 +0000' git commit -qam "Adding fileF"
# Visualize the graph to verify its the right shape
  $ git log --all --decorate --oneline --graph
  * fc69d5f (HEAD -> master_bookmark) Adding fileF
  * 5c4d2c0 Adding fileE
  * c02746f Adding fileC and fileD
  * 4680829 Adding fileA and fileB

# Perform a shallow clone of the repo with commits created only after 02/01/0000 00:01. NOTE: git looks at the committer date NOT the author date
  $ cd "$TESTTMP"
  $ git clone --shallow-since='02/01/0000 00:01 +0000' file://"$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  $ cd $GIT_REPO
# Visualize the graph to verify its the right shape
  $ git log --all --decorate --oneline --graph 
  * fc69d5f (HEAD -> master_bookmark, origin/master_bookmark, origin/HEAD) Adding fileF
  * 5c4d2c0 (grafted) Adding fileE

# Import the repo into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO_ORIGIN" --derive-hg --generate-bookmarks full-repo

# Start up the Mononoke Git Service
  $ mononoke_git_service

# Perform Mononoke clone with the depth of 3 and it should have the expected output
  $ cd "$TESTTMP"
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --shallow-since='02/01/0000 00:01 +0000'
  $ cd $REPONAME
# Visualize the graph to verify its the right shape
  $ git log --all --decorate --oneline --graph
  * fc69d5f (HEAD -> master_bookmark, origin/master_bookmark, origin/HEAD) Adding fileF
  * 5c4d2c0 (grafted) Adding fileE
# Delete both clone and reclone with different shallow constraint
  $ cd "$TESTTMP"
  $ rm -rf $REPONAME
  $ rm -rf $GIT_REPO

# Perform a shallow clone of the repo with commits created only after 03/01/0000 00:01. NOTE: git looks at the committer date NOT the author date
  $ cd "$TESTTMP"
  $ git clone --shallow-since='03/01/0000 00:00 +0000' file://"$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  $ cd $GIT_REPO
# Visualize the graph to verify its the right shape
  $ git log --all --decorate --oneline --graph 
  * fc69d5f (HEAD -> master_bookmark, origin/master_bookmark, origin/HEAD) Adding fileF
  * 5c4d2c0 (grafted) Adding fileE
# Perform Mononoke clone with the depth of 3 and it should have the expected output
  $ cd "$TESTTMP"
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --shallow-since='02/01/0000 00:01 +0000'
  $ cd $REPONAME
# Visualize the graph to verify its the right shape
  $ git log --all --decorate --oneline --graph
  * fc69d5f (HEAD -> master_bookmark, origin/master_bookmark, origin/HEAD) Adding fileF
  * 5c4d2c0 (grafted) Adding fileE
