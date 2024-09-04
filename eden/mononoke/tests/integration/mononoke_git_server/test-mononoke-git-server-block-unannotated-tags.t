# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO_SUBMODULE="${TESTTMP}/origin/repo-submodule-git"
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ cat >> repos/repo/server.toml <<EOF
  > [source_control_service]
  > permit_writes = true
  > EOF

  $ cat >> repos/repo/server.toml <<EOF
  > [[bookmarks]]
  > regex=".*"
  > [[bookmarks.hooks]]
  > hook_name="block_unannotated_tags"
  > [[hooks]]
  > name="block_unannotated_tags"
  > config_json='{}'
  > EOF

# Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ old_head=$(git rev-parse HEAD)
  $ git tag -a -m"new tag" first_tag
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
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
  Cloning into 'repo'...

# Add some new commits to the cloned repo and push it to remote
  $ cd repo
  $ git tag completely_new_tag

# Push unannotated tag - should fail
  $ git_client push origin completely_new_tag
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] completely_new_tag -> completely_new_tag (hooks failed:
    block_unannotated_tags for e8615d6f149b876be0a2f30a1c5bf0c42bf8e136: The un-annotated tag "tags/completely_new_tag" is not allowed in this repository.
  Use 'git tag [ -a | -s ]' for tags you want to propagate.
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]

# Push annotated tag - should succeed
  $ git tag -a -m "note" completely_new_annotated_tag
  $ git_client push origin completely_new_annotated_tag
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   * [new tag]         completely_new_annotated_tag -> completely_new_annotated_tag
