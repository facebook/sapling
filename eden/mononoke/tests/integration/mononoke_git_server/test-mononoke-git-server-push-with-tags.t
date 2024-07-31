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

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO" --derive-hg --generate-bookmarks full-repo

# Set Mononoke as the Source of Truth
  $ set_mononoke_as_source_of_truth_for_git

# Ensure that we have entry in bonsai_tag_mapping table for the pushed tags
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT tag_name, hex(changeset_id) as cs_id, hex(tag_hash) as tag_hash, target_is_tag FROM bonsai_tag_mapping ORDER BY tag_name"
  tags/first_tag|5CA579C0E3EBEA708371B65CE559E5A51B231AD1B6F3CDFD874CA27362A2A6A8|8963E1F55D1346A07C3AEC8C8FC72BF87D0452B1|0
  tags/second_tag|B40C7E078B46D907C6679AA511B981242845EB3D3F7AF3719B863E8833503EFA|CE5A26BA55C422E8E3960224153EF5CF35E75B14|0

# Ensure that the tags show up in bookmarks table
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT name, hex(changeset_id) FROM bookmarks WHERE name LIKE 'tags/%' ORDER BY name"
  tags/first_tag|032CD4DCE0406F1C1DD1362B6C3C9F9BDFA82F2FC5615E237A890BE4FE08B044
  tags/second_tag|DA93DC81BADD8D407DB0F3219EC0EC78F1EF750EBFA95735BB483310371AF80C

# Start up the Mononoke Git Service
  $ mononoke_git_service
# Clone the Git repo from Mononoke
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
  Cloning into 'repo'...
# List all the known refs. Note that both tags are present and pointing to specific commits
  $ cd repo
  $ git show-ref | grep tags | sort
  8963e1f55d1346a07c3aec8c8fc72bf87d0452b1 refs/tags/first_tag
  ce5a26ba55c422e8e3960224153ef5cf35e75b14 refs/tags/second_tag

# Update one tag and delete one tag
  $ echo "this is file3" > file3
  $ git add file3
  $ git commit -qam "Add file3"
  $ git tag -a -f -m "recreated tag still called first_tag" first_tag
  Updated tag 'first_tag' (was 8963e1f)
  $ git tag -d second_tag
  Deleted tag 'second_tag' (was ce5a26b)

# Try to push the updated tag, it should fail tag changes are not allowed once the tag is created on the remote
  $ git_client push origin --tags
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [rejected]        first_tag -> first_tag (already exists)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  hint: Updates were rejected because the tag already exists in the remote.
  [1]

# Delete all known tags and then push the updated tag to the server
  $ git_client push origin --delete first_tag second_tag
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   - [deleted]         *_tag (glob)
   - [deleted]         *_tag (glob)

# Wait for the warm bookmark cache to catch up with the latest changes 
  $ wait_for_git_bookmark_delete refs/tags/first_tag

# This time the push of updated tag should succeed
  $ git_client push origin --tags
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   * [new tag]         first_tag -> first_tag

# Wait for the warm bookmark cache to catch up with the latest changes 
  $ wait_for_git_bookmark_create refs/tags/first_tag

# Clone the repo in a new folder
  $ cd "$TESTTMP"
  $ git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git new_repo
  Cloning into 'new_repo'...
  $ cd new_repo

# List all the known refs. Ensure that the deleted tag do not show up anymore and the updated tag
# points to a different commit than before
  $ git show-ref | grep tags | sort
  5c160d85002e94d3583b660cc3689a820ef7379d refs/tags/first_tag

# Ensure that we have entry in bonsai_tag_mapping table for just first_tag that is different than the one before
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT tag_name, hex(changeset_id) as cs_id, hex(tag_hash) as tag_hash, target_is_tag FROM bonsai_tag_mapping ORDER BY tag_name"
  tags/first_tag|CE935806373A5F7B912F34D2B1AD35CF5897B6EFA98D7ECDF366A601AE250DB7|5C160D85002E94D3583B660CC3689A820EF7379D|0

# Ensure that only the first_tag show up in bookmarks table pointing to a different commit than before
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT name, hex(changeset_id) FROM bookmarks WHERE name LIKE 'tags/%' ORDER BY name"
  tags/first_tag|E70BAE430CAF70F469DD5517D77F396290A0CAD67AF436397849F01075413ED2
