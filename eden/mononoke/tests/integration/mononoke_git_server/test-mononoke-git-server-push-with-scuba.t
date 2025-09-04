# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ export COMMIT_SCRIBE_CATEGORY=mononoke_commits
  $ export BOOKMARK_SCRIBE_CATEGORY=mononoke_bookmark
  $ export WBC_SCRIBE_CATEGORY=mononoke_bookmark
  $ export TAGS_SCRIBE_CATEGORY=mononoke_bookmark
  $ export MONONOKE_TEST_SCRIBE_LOGGING_DIRECTORY=$TESTTMP/scribe_logs/
  $ . "${TEST_FIXTURES}/library.sh"
  $ export ENABLE_BOOKMARK_CACHE=1
  $ REPOTYPE="blob_files"
  $ export ONLY_FAST_FORWARD_BOOKMARK_REGEX=".*ffonly"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ SCUBA="$TESTTMP/scuba.json"

  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:wbc_update_by_scribe_tailer": true
  >   }
  > }
  > EOF
  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:enable_bonsai_tag_mapping_caching": true
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
# Create a tag pointing to the first commit
  $ git tag -a -m "new tag" first_tag
  $ current_first_tag=$(git rev-parse HEAD)
  $ echo "this is file2" > file2
  $ git add file2
  $ git commit -qam "Add file2"
# Create another tag pointing to the second commit
  $ git tag -a -m "second tag" second_tag

  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.

# Enable logging of bookmark updates
  $ mkdir -p $TESTTMP/scribe_logs
  $ touch $TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO" --derive-hg --generate-bookmarks full-repo

# Set Mononoke as the Source of Truth
  $ set_mononoke_as_source_of_truth_for_git

# Start up the Mononoke Git Service
  $ mononoke_git_service
# Clone the Git repo from Mononoke
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git  
  $ cd repo


# Add some new commits to the cloned repo and push it to remote
  $ current_head=$(git rev-parse HEAD)
  $ echo "newly added file" > new_file
  $ git add .
  $ git commit -qam "Commit with newly added file"
  $ git checkout -b new_branch
  Switched to a new branch 'new_branch'
  $ echo "new file on new branch" > another_new_file
  $ git add .
  $ git commit -qam "New commit on new branch"
  $ git tag -a -m "Tag for push" push_tag
  $ git tag -a -m "Tag pointing in the past" past_tag $old_head

# Push all the changes made so far
  $ git_client push origin --all --follow-tags
  To https://*/repos/git/ro/repo.git (glob)
     e8615d6..e8b927e  master_bookmark -> master_bookmark
   * [new branch]      new_branch -> new_branch
   * [new tag]         past_tag -> past_tag
   * [new tag]         push_tag -> push_tag

# Wait for the warm bookmark cache to catch up with the latest changes
  $ wait_for_git_bookmark_move HEAD $current_head
  $ wait_for_git_bookmark_create refs/heads/new_branch
  $ wait_for_git_bookmark_create refs/tags/push_tag
  $ wait_for_git_bookmark_create refs/tags/past_tag

# Verify that we log the size of the packfile used in push
  $ jq -S .int "$SCUBA" | grep -e "packfile_size"
    "packfile_size": 1173,

# Verify the timed futures logged with log tags show up in scuba logs
  $ jq .normal "$SCUBA" | grep -e "Packfile" -e "GitImport" -e "Bookmark movement" -e "Prerequisite" -e "Objects" -e "Prefetched" -e "Content Blob" -e "Bonsai Changeset" -e "Finalize Batch" -e "Push" -e "Import" | sort
    "log_tag": "Bookmark movement completed",
    "log_tag": "Completed Bonsai Changeset creation for all commits",
    "log_tag": "Completed Finalize Batch for all commits",
    "log_tag": "Completed Finalize Batch",
    "log_tag": "Created Bonsai Changeset for Git Commit",
    "log_tag": "Created Bonsai Changeset for Git Commit",
    "log_tag": "Decoded objects from Packfile",
    "log_tag": "Fetched Prerequisite Objects",
    "log_tag": "GitImport, Derivation and Bonsai creation completed",
    "log_tag": "Packfile stats",
    "log_tag": "Parsed complete Packfile",
    "log_tag": "Prefetched existing BonsaiGit Mappings",
    "log_tag": "Sent Packfile OK",
    "log_tag": "Uploaded Content Blob, Git Blob, Commits and Trees for all commits",
    "log_tag": "Uploaded Content Blob, Git Blob, Commits and Trees",
    "log_tag": "Uploaded Content Blob, Git Blob, Commits and Trees",
    "log_tag": "Uploaded Content Blob, Git Blob, Commits and Trees",
    "log_tag": "Verified Packfile Checksum",
    "msg": "Import",
    "msg": "Import",
    "msg": "Import",
    "msg": "Import",
    "msg": "Import",
    "msg": "Import",
    "msg": "Import",
    "msg": "Import",
    "msg": "Import",
    "msg": "Import",
    "msg": "Push",
    "msg": "Push",
    "msg": "Push",
    "msg": "Push",
    "msg": "Push",
    "msg": "Push",
    "msg": "Push",
