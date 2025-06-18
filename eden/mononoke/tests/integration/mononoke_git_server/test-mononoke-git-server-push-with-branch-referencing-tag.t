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
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ old_head=$(git rev-parse HEAD)
  $ git tag -a -m "new tag" first_tag
  $ echo "this is file2" > file2
  $ git add file2
  $ git commit -qam "Add file2"
  $ git tag -a empty_tag -m ""
  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO" --derive-hg --generate-bookmarks full-repo

# Set Mononoke as the Source of Truth
  $ set_mononoke_as_source_of_truth_for_git

# Start up the Mononoke Git Service
  $ mononoke_git_service
# Clone the Git repo from Mononoke
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git

# Push a branch with the ref-spec referencing a tag name instead of a commit hash
  $ cd repo

# Push all the changes made so far
  $ git_client push origin first_tag:refs/heads/new_bookmark
  To https://*/repos/git/ro/repo.git (glob)
   * [new branch]      first_tag -> new_bookmark

# Wait for the warm bookmark cache to catch up with the latest changes
  $ wait_for_git_bookmark_create refs/heads/new_bookmark

# Cloning the repo in a new folder
  $ cd "$TESTTMP"
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git new_repo
  $ cd new_repo

# List all the known refs. Note that the branch new_bookmark is now parsed correctly as a branch and not as a tag
  $ git show-ref -d | sort
  8963e1f55d1346a07c3aec8c8fc72bf87d0452b1 refs/tags/first_tag
  8ce3eae44760b500bf3f2c3922a95dcd3c908e9e refs/remotes/origin/new_bookmark
  8ce3eae44760b500bf3f2c3922a95dcd3c908e9e refs/tags/first_tag^{}
  e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/heads/master_bookmark
  e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/remotes/origin/HEAD
  e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/remotes/origin/master_bookmark
  e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/tags/empty_tag^{}
  fb02ed046a1e75fe2abb8763f7c715496ae36353 refs/tags/empty_tag

# The bonsai tag mapping table also confirms that we do not treat new_bookmark as a tag
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT tag_name, hex(changeset_id) as cs_id, hex(tag_hash) as tag_hash, target_is_tag FROM bonsai_tag_mapping ORDER BY tag_name"
  tags/empty_tag|1910A71753B6A3F0A308C44E85AE28EB57272D5519D53C4577AF4395784EFDB3|FB02ED046A1E75FE2ABB8763F7C715496AE36353|0
  tags/first_tag|5CA579C0E3EBEA708371B65CE559E5A51B231AD1B6F3CDFD874CA27362A2A6A8|8963E1F55D1346A07C3AEC8C8FC72BF87D0452B1|0
