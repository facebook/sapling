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
# Create few commits with different commit times where the committer dates aren't monotonic
  $ echo "File A.File A.File A.File A.File A.File A.File A.File A.File A.File A.File A.File A.File A.File A.File A.File A." > fileA
  $ echo "File B.File B.File B.File B.File B.File B.File B.File B.File B.File B.File B.File B.File B.File B.File B.File B." > fileB
  $ git add .
  $ GIT_AUTHOR_DATE='05/05/0000 00:00 +0000' GIT_COMMITTER_DATE='05/01/0000 00:00 +0000' git commit -qam "Adding fileA and fileB"

  $ echo "File C.File C.File C.File C.File C.File C.File C.File C.File C.File C.File C.File C.File C.File C.File C.File C." > fileC
  $ echo "File D.File D.File D.File D.File D.File D.File D.File D.File D.File D.File D.File D.File D.File D.File D.File D." > fileD
  $ git add .
  $ GIT_AUTHOR_DATE='05/05/0000 00:00 +0000' GIT_COMMITTER_DATE='01/01/0000 00:00 +0000' git commit -qam "Adding fileC and fileD"

  $ echo "File E.File E.File E.File E.File E.File E.File E.File E.File E.File E.File E.File E.File E.File E.File E.File E." > fileE
  $ git add .
  $ GIT_AUTHOR_DATE='05/05/0000 00:00 +0000' GIT_COMMITTER_DATE='04/01/0000 00:00 +0000' git commit -qam "Adding fileE" 

  $ echo "File F.File F.File F.File F.File F.File F.File F.File F.File F.File F.File F.File F.File F.File F.File F.File F." > fileF
  $ git add .
  $ GIT_AUTHOR_DATE='05/05/0000 00:00 +0000' GIT_COMMITTER_DATE='02/01/0000 00:00 +0000' git commit -qam "Adding fileF"

  $ echo "File G.File G.File G.File G.File G.File G.File G.File G.File G.File G.File G.File G.File G.File G.File G.File G." > fileG
  $ git add .
  $ GIT_AUTHOR_DATE='05/05/0000 00:00 +0000' GIT_COMMITTER_DATE='03/01/0000 00:00 +0000' git commit -qam "Adding fileG"
# Visualize the graph to verify its the right shape
  $ git log --all --decorate --oneline --graph
  * 99e0298 (HEAD -> master_bookmark) Adding fileG
  * 5ca22c0 Adding fileF
  * 287dd20 Adding fileE
  * 72dc9d2 Adding fileC and fileD
  * 71cc738 Adding fileA and fileB

# Perform a shallow clone of the repo with commits created only after 02/01/0000 00:01
  $ cd "$TESTTMP"
  $ git clone --shallow-since='02/01/0000 00:01 +0000' file://"$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  $ cd $GIT_REPO
# Visualize the graph to verify its the right shape. Notice that even though there are multiple commits
# that are greater than the input timestamp, the repo just contains 99e0298
  $ git log --all --decorate --oneline --graph 
  * 99e0298 (grafted, HEAD -> master_bookmark, origin/master_bookmark, origin/HEAD) Adding fileG

# Perform a shallow clone of the repo with commit created only after 03/01/0000 00:01
  $ cd "$TESTTMP"
  $ rm -rf $GIT_REPO
  $ git clone --shallow-since='03/01/0000 00:01 +0000' file://"$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  fatal: no commits selected for shallow requests
  fatal: the remote end hung up unexpectedly
  [128]

# Perform a shallow clone of the repo with commit created only after 01/01/0000 00:01
  $ cd "$TESTTMP"
  $ rm -rf $GIT_REPO
  $ git clone --shallow-since='01/01/0000 00:01 +0000' file://"$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  $ cd $GIT_REPO
  $ git log --all --decorate --oneline --graph 
  * 99e0298 (HEAD -> master_bookmark, origin/master_bookmark, origin/HEAD) Adding fileG
  * 5ca22c0 Adding fileF
  * 287dd20 (grafted) Adding fileE

# Import the repo into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO_ORIGIN" --derive-hg --generate-bookmarks full-repo

# Start up the Mononoke Git Service
  $ mononoke_git_service

# Perform Mononoke clone of the repo with commits created only after 02/01/0000 00:01
  $ cd "$TESTTMP"
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --shallow-since='02/01/0000 00:01 +0000'
  $ cd $REPONAME
  $ git log --all --decorate --oneline --graph 
  * 99e0298 (grafted, HEAD -> master_bookmark, origin/master_bookmark, origin/HEAD) Adding fileG

# Perform a Mononoke shallow clone of the repo with commit created only after 01/01/0000 00:01
  $ cd "$TESTTMP"
  $ rm -rf $REPONAME
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --shallow-since='01/01/0000 00:01 +0000'
  $ cd $REPONAME
  $ git log --all --decorate --oneline --graph
  * 99e0298 (HEAD -> master_bookmark, origin/master_bookmark, origin/HEAD) Adding fileG
  * 5ca22c0 Adding fileF
  * 287dd20 (grafted) Adding fileE

# Perform Mononoke clone of the repo with commits created only after 03/01/0000 00:01
  $ cd "$TESTTMP"
  $ rm -rf $REPONAME
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --shallow-since='03/01/0000 00:01 +0000'
  Cloning into 'repo'...
  fatal: expected 'packfile', received '?Failed to generate shallow info
  
  Caused by:
      0: Error in getting ancestors after time during shallow-info
      1: No commits selected for shallow requests with committer time greater than 951868860'
  [128]

#  $ cd $REPONAME
# Validate that the list of commits returned match the expected output
#  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | grep commit | sort
