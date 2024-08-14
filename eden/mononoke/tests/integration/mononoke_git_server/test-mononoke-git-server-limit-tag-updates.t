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
  > hook_name="limit_tag_updates"
  > [[hooks]]
  > name="limit_tag_updates"
  > config_json='{}'
  > EOF

  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:run_hooks_on_additional_changesets": true
  >   }
  > }
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
  $ git tag -a -f -m "recreated tag still called first_tag" first_tag
  Updated tag 'first_tag' (was 8963e1f)

# Push the tag on a new commit
  $ git_client push --tags --force
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   + e8615d6...fb02ed0 empty_tag -> empty_tag (forced update)
   ! [remote rejected] first_tag -> first_tag (hooks failed:
    limit_tag_updates for *: Commit edits a tag tags/first_tag (glob)
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]
