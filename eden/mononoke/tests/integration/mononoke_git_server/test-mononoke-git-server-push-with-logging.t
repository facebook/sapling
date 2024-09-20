# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ export COMMIT_SCRIBE_CATEGORY=mononoke_commits
  $ export BOOKMARK_SCRIBE_CATEGORY=mononoke_bookmark
  $ export MONONOKE_TEST_SCRIBE_LOGGING_DIRECTORY=$TESTTMP/scribe_logs/
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
  $ old_head=$(git rev-parse HEAD)
  $ git tag -a -m "new tag" first_tag
  $ git checkout -b branch_to_be_deleted
  Switched to a new branch 'branch_to_be_deleted'
  $ echo "file to be deleted" > deleted_file
  $ git add .
  $ git commit -qam "Deleted file commit"
  $ git checkout master
  Switched to branch 'master'
  $ echo "this is file2" > file2
  $ git add .
  $ git commit -qam "Add file2"
  $ git tag -a empty_tag -m ""
  $ cd "$TESTTMP"
  $ git clone --mirror "$GIT_REPO_ORIGIN" repo-git
  Cloning into bare repository 'repo-git'...
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
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
  Cloning into 'repo'...

# Add some new commits to the cloned repo and push it to remote
  $ cd repo
  $ current_head=$(git rev-parse HEAD)
  $ echo "Adding another file to the mix" > mix_file
  $ git add .
  $ git commit -qam "Commit with added mix_file"
# Create a new branch
  $ git checkout -b new_branch
  Switched to a new branch 'new_branch'
  $ echo "Content for new branch" > new_file
  $ git add .
  $ git commit -qam "Commit for new branch"
# Delete an existing branch
  $ git branch -d branch_to_be_deleted
  error: branch 'branch_to_be_deleted' not found* (glob)
  [1]

# Push all the changes made so far
  $ git_client push origin --all --follow-tags
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
     e8615d6..84bcc24  master -> master
   * [new branch]      new_branch -> new_branch
  $ git_client push origin --delete branch_to_be_deleted
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   - [deleted]         branch_to_be_deleted

# Wait for WBC to catch up with the changes
  $ wait_for_git_bookmark_delete refs/heads/branch_to_be_deleted

# Validate if the bookmark moves got logged by Mononoke Bookmark logger and GitRefs logger. Mononoke Bookmark logger doesn't
# use refs/ as prefix and stores unspecified commits as null. GitRefs logger uses refs/ prefix, stores unspecified commits
# as git null hash and appends .git suffix to repo name
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | sort | jq '{repo_name,bookmark_name,old_bookmark_value,new_bookmark_value,operation}'
  {
    "repo_name": "repo",
    "bookmark_name": "heads/branch_to_be_deleted",
    "old_bookmark_value": null,
    "new_bookmark_value": "a128e37df7b19af47d7698b76b0fff345b2d92eeb390fa46b211f296ede95c97",
    "operation": "create"
  }
  {
    "repo_name": "repo",
    "bookmark_name": "heads/branch_to_be_deleted",
    "old_bookmark_value": "a128e37df7b19af47d7698b76b0fff345b2d92eeb390fa46b211f296ede95c97",
    "new_bookmark_value": null,
    "operation": "delete"
  }
  {
    "repo_name": "repo",
    "bookmark_name": "heads/master",
    "old_bookmark_value": null,
    "new_bookmark_value": "da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c",
    "operation": "create"
  }
  {
    "repo_name": "repo",
    "bookmark_name": "heads/master",
    "old_bookmark_value": "da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c",
    "new_bookmark_value": "a2b9a16e20a9a328b497a5ba9d6a20285688795410225b98f34bfbacd73370b8",
    "operation": "update"
  }
  {
    "repo_name": "repo",
    "bookmark_name": "heads/new_branch",
    "old_bookmark_value": null,
    "new_bookmark_value": "0588bf8ffa39951e255c0881d5df9faf6880780d02b022882072f3acf0c7b69b",
    "operation": "create"
  }
  {
    "repo_name": "repo",
    "bookmark_name": "tags/empty_tag",
    "old_bookmark_value": null,
    "new_bookmark_value": "da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c",
    "operation": "create"
  }
  {
    "repo_name": "repo",
    "bookmark_name": "tags/first_tag",
    "old_bookmark_value": null,
    "new_bookmark_value": "032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044",
    "operation": "create"
  }
  {
    "repo_name": "repo.git",
    "bookmark_name": "refs/heads/branch_to_be_deleted",
    "old_bookmark_value": "2e70fb91397e9861fa3afd7c981c10f903fc352b",
    "new_bookmark_value": "0000000000000000000000000000000000000000",
    "operation": null
  }
  {
    "repo_name": "repo.git",
    "bookmark_name": "refs/heads/master",
    "old_bookmark_value": "e8615d6f149b876be0a2f30a1c5bf0c42bf8e136",
    "new_bookmark_value": "84bcc2429ed75bca1a1c8c98fc49f283b69e333a",
    "operation": null
  }
  {
    "repo_name": "repo.git",
    "bookmark_name": "refs/heads/new_branch",
    "old_bookmark_value": "0000000000000000000000000000000000000000",
    "new_bookmark_value": "f03735df9cbce16686a56086146037ef0c54d8a7",
    "operation": null
  }
