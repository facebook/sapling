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
# Even though the shallow clone fetched only the head commit, it would contain the full-working copy data at that
# commit
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | grep -e tree -e blob | sort 
  0b80b08378adc84a9b2739e414c2d5f8df947286 blob fileC
  4e9bb9e1cf18cbee69fa5b7313565bda3efa56a1 tree 
  5f4d71f75dafdb94c0ba7a13c14c90ecbf486208 blob fileA
  74ab5365c4814d39e90ab2217b03cf456493aea8 blob fileF
  83fdad59d2ec3c08a2cafba805b1ccd8b695131b blob fileB
  95adcfd5bff1763314117f9ee9f65fe031e208b6 blob fileD
  a35b62718902b7abac8d19d32015889f114addaa blob fileE

# Perform a shallow pull of the repo which deeper than the clone (i.e. depth = 3). This should result in
# unshallowing of the repo at the client
  $ git pull --depth=3
  Already up to date.
# Even though the above pull outputs `Already up to date`, we end up fetching additional commits based on depth
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | grep commit
  18a6f40de35ce474e240faa7298ae2b5979751c8 commit 
  9089a8c5d6429a5dfa430d1abefd73234894c4df commit 
  619f44e4b1883ec6cafa608967d2f314f2224792 commit 
  12a34ee8026e5118cf6a2123c94057d1c8f9c5bb commit 
  a9ff5f932c4a81f710d754b02e20dcbb8236cc23 commit 

# Import the repo into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO_ORIGIN" --derive-hg --generate-bookmarks full-repo

# Start up the Mononoke Git Service
  $ mononoke_git_service

# Perform a similar shallow clone using Mononoke
  $ cd "$TESTTMP"
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --depth=1
  Cloning into 'repo'...
  $ cd $REPONAME
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | grep commit | sort
  18a6f40de35ce474e240faa7298ae2b5979751c8 commit 
# Even though the shallow clone fetched only the head commit, it would contain the full-working copy data at that
# commit
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | grep -e tree -e blob | sort 
  0b80b08378adc84a9b2739e414c2d5f8df947286 blob fileC
  4e9bb9e1cf18cbee69fa5b7313565bda3efa56a1 tree 
  5f4d71f75dafdb94c0ba7a13c14c90ecbf486208 blob fileA
  74ab5365c4814d39e90ab2217b03cf456493aea8 blob fileF
  83fdad59d2ec3c08a2cafba805b1ccd8b695131b blob fileB
  95adcfd5bff1763314117f9ee9f65fe031e208b6 blob fileD
  a35b62718902b7abac8d19d32015889f114addaa blob fileE

# Perform a shallow pull of the repo which deeper than the clone (i.e. depth = 3). This should result in
# unshallowing of the repo at the client
  $ git_client pull --depth=3
  Already up to date.
# Even though the above pull outputs `Already up to date`, we end up fetching additional commits based on depth
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | grep commit
  18a6f40de35ce474e240faa7298ae2b5979751c8 commit 
  9089a8c5d6429a5dfa430d1abefd73234894c4df commit 
  619f44e4b1883ec6cafa608967d2f314f2224792 commit 
  12a34ee8026e5118cf6a2123c94057d1c8f9c5bb commit 
  a9ff5f932c4a81f710d754b02e20dcbb8236cc23 commit 
