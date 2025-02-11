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

# Import the repo into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO_ORIGIN" --derive-hg --generate-bookmarks full-repo

# Set Mononoke as the Source of Truth
  $ set_mononoke_as_source_of_truth_for_git

# Start up the Mononoke Git Service
  $ mononoke_git_service

# Perform Mononoke clone with the depth of 1
  $ cd "$TESTTMP"
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --depth=1
# Push to Mononoke from shallow cloned repo
  $ cd $REPONAME
  $ current_head=$(git rev-parse HEAD)
  $ echo "newly added file" > new_file
  $ git add .
  $ git commit -qam "Commit with newly added file"
  $ git_client push origin --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     18a6f40..3b8e8f3  R2 -> R2
