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
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ git tag -a -m "new tag" first_tag
  $ echo "this is file2" > file2
  $ git add file2
  $ git commit -qam "Add file2"
# Create a new branch with a commit
  $ git checkout -b new_branch
  Switched to a new branch 'new_branch'
  $ echo "new file on new branch" > another_new_file
  $ git add .
  $ git commit -qam "New commit on new branch"
# Create another new branch with a commit
  $ git checkout -b another_branch
  Switched to a new branch 'another_branch'
  $ echo "another newly added file" > another_new_file
  $ git add .
  $ git commit -qam "Commit with another newly added file"
# Create yet another new branch with a commit
  $ git checkout -b yet_another_branch
  Switched to a new branch 'yet_another_branch'
  $ echo "yet another newly added file" > yet_another_new_file
  $ git add .
  $ git commit -qam "Commit with yet another newly added file"
  $ git checkout master
  Switched to branch 'master'

  $ cd "$TESTTMP"
  $ git clone --mirror "$GIT_REPO_ORIGIN" repo-git
  Cloning into bare repository 'repo-git'...
  done.

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO" --derive-hg --generate-bookmarks full-repo

# Set Mononoke as the Source of Truth
  $ set_mononoke_as_source_of_truth_for_git

# Start up the Mononoke Git Service
  $ mononoke_git_service
# Clone the Git repo from Mononoke
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
  Cloning into 'repo'...

# Delete some branches and push to remote
  $ cd repo
  $ git_client -c http.extraHeader="x-git-read-after-write-consistency: 1" push origin --delete new_branch another_branch
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   - [deleted]         *_branch (glob)
   - [deleted]         *_branch (glob)

# Without waiting for the WBC, clone the repo and check its state
  $ cd "$TESTTMP"
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git new_repo
  Cloning into 'new_repo'...
  $ cd new_repo

# List all the known refs. Ensure that the deleted branches do not show up anymore
  $ git show-ref | sort
  8963e1f55d1346a07c3aec8c8fc72bf87d0452b1 refs/tags/first_tag
  e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/heads/master
  e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/remotes/origin/HEAD
  e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/remotes/origin/master
  fc3048042ba6628ce96a6a6ce7d1839327ec4563 refs/remotes/origin/yet_another_branch

# Create new branch in the repo
  $ cd "$TESTTMP"/repo
  $ git checkout -b brand_new_branch master
  Switched to a new branch 'brand_new_branch'
  $ echo "File on brand new branch" > brand_new_branch_file.txt
  $ git add .
  $ git commit -qam "Brand new branch commit"
  $ git_client -c http.extraHeader="x-git-read-after-write-consistency: 1" push origin brand_new_branch
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   * [new branch]      brand_new_branch -> brand_new_branch

# Without waiting for the WBC, clone the repo and check its state
  $ cd "$TESTTMP"
  $ rm -rf new_repo
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git new_repo
  Cloning into 'new_repo'...
  $ cd new_repo

# List all the known refs. Ensure that the newly created branch is present
  $ git show-ref | sort
  55e2d4267a1afd04875670380119e989c8e0bf97 refs/remotes/origin/brand_new_branch
  8963e1f55d1346a07c3aec8c8fc72bf87d0452b1 refs/tags/first_tag
  e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/heads/master
  e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/remotes/origin/HEAD
  e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/remotes/origin/master
  fc3048042ba6628ce96a6a6ce7d1839327ec4563 refs/remotes/origin/yet_another_branch

# Modify existing branches in the repo
  $ cd "$TESTTMP"/repo
  $ git checkout master
  Switched to branch 'master'
  Your branch is up to date with 'origin/master'.
  $ echo "Moving master file" > file1
  $ git add .
  $ git commit -qam "Moving master branch"
  $ git checkout yet_another_branch
  Switched to a new branch 'yet_another_branch'
  ?ranch 'yet_another_branch' set up to track *yet_another_branch*. (glob)
  $ echo "Moving yet another file" > yet_another_new_file
  $ git add .
  $ git commit -qam "Moving yet another branch"
  $ git_client -c http.extraHeader="x-git-read-after-write-consistency: 1" push origin --all
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     e8615d6..bbc5b18  master -> master
     fc30480..afa25c0  yet_another_branch -> yet_another_branch

# Without waiting for the WBC, clone the repo and check its state
  $ cd "$TESTTMP"
  $ rm -rf new_repo
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git new_repo
  Cloning into 'new_repo'...
  $ cd new_repo

# List all the known refs. Ensure that the modified branches show their changes
  $ git show-ref | sort
  55e2d4267a1afd04875670380119e989c8e0bf97 refs/remotes/origin/brand_new_branch
  8963e1f55d1346a07c3aec8c8fc72bf87d0452b1 refs/tags/first_tag
  afa25c08df81858debe9dcede1bc5de3f2512a08 refs/remotes/origin/yet_another_branch
  bbc5b18b3fdb6465d94ad062818434a1aa9b1bbb refs/heads/master
  bbc5b18b3fdb6465d94ad062818434a1aa9b1bbb refs/remotes/origin/HEAD
  bbc5b18b3fdb6465d94ad062818434a1aa9b1bbb refs/remotes/origin/master
