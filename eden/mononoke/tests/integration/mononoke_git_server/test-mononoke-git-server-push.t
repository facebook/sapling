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

# Start up the Mononoke Git Service
  $ mononoke_git_service
# Clone the Git repo from Mononoke
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
  Cloning into 'repo'...

# Add some new commits to the cloned repo and push it to remote
  $ cd repo
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

# The git-receive-pack endpoint accepts pushes without moving the bookmarks in the backend
# but stores all the git and bonsai objects in the server
  $ git_client push origin --all --follow-tags
  To https://*/repos/git/ro/repo.git (glob)
     e8615d6..e8b927e  master -> master
   * [new branch]      new_branch -> new_branch
   * [new tag]         past_tag -> past_tag
   * [new tag]         push_tag -> push_tag

# Ensure that all the pushed objects made it to the server
  $ ls $TESTTMP/blobstore/blobs | grep "git_object" | wc -l
  12

# Ensure that all the packfile base items corresponding to those objects made it to the server
  $ ls $TESTTMP/blobstore/blobs | grep "git_packfile_base_item" | wc -l
  16

# Ensure that we have entry in bonsai_tag_mapping table for the pushed tags
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT tag_name, hex(changeset_id) as cs_id, hex(tag_hash) as tag_hash, target_is_tag FROM bonsai_tag_mapping ORDER BY tag_name"
  tags/empty_tag|D5BE6FDF77FC73EE5E3A4BAB1ADBB4772829E06C0F104E6CC0D70CABF1EBFF4B|FB02ED046A1E75FE2ABB8763F7C715496AE36353|0
  tags/first_tag|5CA579C0E3EBEA708371B65CE559E5A51B231AD1B6F3CDFD874CA27362A2A6A8|8963E1F55D1346A07C3AEC8C8FC72BF87D0452B1|0
  tags/past_tag|42DD560265FB5465B9D5B66265B6C50C4B23A13F503ACAA63181A23CCD7CDB1E|9183B513199288101E5AAFB7F5F90B64092093DE|0
  tags/push_tag|04189410E1F520E08AAA430592C5F2B3DD2746AFBCE5DE80A1282ECA10B36A6E|EC8F5A7483999D8D78203A64786F3734D7737EE7|0

# Cloning the repo in a new folder will not get the latest changes since we didn't really accept the push
  $ cd "$TESTTMP"
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git new_repo
  Cloning into 'new_repo'...
  $ cd new_repo

# When trying to list refs, note that new_branch is not present since the server never accepted it
  $ git show-ref | sort
  8963e1f55d1346a07c3aec8c8fc72bf87d0452b1 refs/tags/first_tag
  e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/heads/master
  e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/remotes/origin/HEAD
  e8615d6f149b876be0a2f30a1c5bf0c42bf8e136 refs/remotes/origin/master
  fb02ed046a1e75fe2abb8763f7c715496ae36353 refs/tags/empty_tag
