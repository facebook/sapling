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
  $ echo "this is fileA" > fileA
  $ git add fileA
  $ git commit -qam "Add fileA"
  $ git tag -a -m"new tag" first_tag
  $ echo "this is fileA.1" > fileA
  $ echo "this is fileB.this is fileB.this is fileB.this is fileB.this is fileB.this is fileB.this is fileB.this is fileB.this is fileBthis is fileB.this is fileB.this is fileB.this is fileBthis is fileBthis is fileBthis is fileB" > fileB
  $ git add .
  $ git commit -qam "Modified fileA -> fileA.1, Add fileB"
  $ echo "this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12" > fileA
  $ git add .
  $ git commit -qam "Modified fileA.1 -> fileA.12"
  $ echo "this is fileB.this is fileB.this is fileB.this is fileB.this is fileB.this is fileB.this is fileB.this is fileB.this is fileBthis is fileB.this is fileB.this is fileB.this is fileBthis is fileBthis is fileBthis is fileBthis is fileBthis is fileBthis is fileBthis is fileBthis is fileBthis is fileBthis is fileBthis is fileBthis is fileB" > fileB
  $ git add .
  $ git commit -qam "Modified fileB"
  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.

# Import the repo into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO_ORIGIN" --derive-hg --generate-bookmarks full-repo

# Set Mononoke as the Source of Truth
  $ set_mononoke_as_source_of_truth_for_git

# Start up the Mononoke Git Service
  $ mononoke_git_service

# Perform Mononoke clone with the depth of 1
  $ cd "$TESTTMP"
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git --depth=1
  Cloning into 'repo'...
# Push to Mononoke from shallow cloned repo
  $ cd $REPONAME
  $ echo "this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12this is fileA.12" > fileA
  $ git add .
  $ git commit -qam "Modified fileA.12"
  $ git_client push origin --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     5b3b465..f3b108f  master_bookmark -> master_bookmark
