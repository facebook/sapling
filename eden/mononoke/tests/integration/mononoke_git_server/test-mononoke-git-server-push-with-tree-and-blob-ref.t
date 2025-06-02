# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ export GIT_CONTENT_REFS_SCRIBE_CATEGORY=mononoke_git_content_refs
  $ export CONTENT_REFS_SCRIBE_CATEGORY=mononoke_git_content_refs
  $ export MONONOKE_TEST_SCRIBE_LOGGING_DIRECTORY=$TESTTMP/scribe_logs/
  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"

  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:enable_git_ref_content_mapping_caching": true
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
  $ git tag -a -m "new tag" first_tag
  $ echo "this is file2" > file2
  $ git add file2
  $ git commit -qam "Add file2"
  $ git tag -a empty_tag -m ""
  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.
  $ cd $GIT_REPO
  $ current_head=$(git rev-parse HEAD)

# Enable logging of content ref updates
  $ mkdir -p $TESTTMP/scribe_logs
  $ touch $TESTTMP/scribe_logs/$GIT_CONTENT_REFS_SCRIBE_CATEGORY

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO" --derive-hg --generate-bookmarks full-repo

# Set Mononoke as the Source of Truth
  $ set_mononoke_as_source_of_truth_for_git

# Start up the Mononoke Git Service
  $ mononoke_git_service
# Clone the Git repo from Mononoke
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git

# Add some new commits to the cloned repo and create a branch and tag
# pointing to non-commit/non-tag objects
  $ cd repo
  $ current_head=$(git rev-parse HEAD)
  $ echo "newly added file" > new_file
  $ git add .
  $ git commit -qam "Commit with newly added file"
# Create a new branch that points to a tree instead of pointing to a commit
  $ echo $(git log --pretty=format:"%T" -1 HEAD) > .git/refs/heads/new_branch
# Create a new branch that points to a blob instead of pointing to a commit
  $ git tag -a push_tag $(git hash-object new_file) -m "Tag for push"
# Capture all the known Git objects from the repo
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/object_list

# Push all the changes made so far
  $ git_client push origin master_bookmark push_tag new_branch
  To https://*/repos/git/ro/repo.git (glob)
     e8615d6..e8b927e  master_bookmark -> master_bookmark
   * [new tag]         push_tag -> push_tag
   * [new branch]      new_branch -> new_branch

# Wait for the WBC to catch up
  $ wait_for_git_bookmark_move HEAD $current_head

# Reclone the repo and validate that we get back all the expected objects
  $ cd $TESTTMP
  $ quiet git_client clone --mirror $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git new_repo
  $ cd new_repo
# Verify that we get the same Git repo back that we started with
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | sort > $TESTTMP/new_object_list
  $ diff -w $TESTTMP/new_object_list $TESTTMP/object_list

# Attempt to delete the content refs. The server should error out with the message that the operation is not supported
  $ cd "$TESTTMP"/repo
  $ git_client push origin --delete push_tag new_branch
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] new_branch (Deletion of refs pointing to trees or blobs (e.g. refs/heads/new_branch) is not permitted in Mononoke Git for repo repo)
   ! [remote rejected] push_tag (Deletion of refs pointing to trees or blobs (e.g. refs/tags/push_tag) is not permitted in Mononoke Git for repo repo)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]

# Validate if the content refs moves got logged by Mononoke Git Content Refs logger
  $ cat "$TESTTMP/scribe_logs/$GIT_CONTENT_REFS_SCRIBE_CATEGORY" | sort | jq '{repo_name,ref_name,git_hash,object_type}'
  {
    "repo_name": "repo",
    "ref_name": "heads/new_branch",
    "git_hash": "215c6866a56fcc82c4b848cc199a4813e4a4c9bf",
    "object_type": "tree"
  }
  {
    "repo_name": "repo",
    "ref_name": "tags/push_tag",
    "git_hash": "37741cfae6de6b691451fee448c5c3e160f454a5",
    "object_type": "blob"
  }
