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
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"

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
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
# List all the known refs. Note that both tags are present and pointing to specific commits
  $ cd repo
  $ git show-ref | grep tags | sort
  8963e1f55d1346a07c3aec8c8fc72bf87d0452b1 refs/tags/first_tag
  ce5a26ba55c422e8e3960224153ef5cf35e75b14 refs/tags/second_tag

# Override one tag, making it a simple tag instead of annotated tag
  $ echo "this is file3" > file3
  $ git add file3
  $ git commit -qam "Add file3"
  $ git tag -f first_tag
  Updated tag 'first_tag' (was 8963e1f)

# Push the updated tag forcefully to the server
  $ git_client push origin --force HEAD:refs/tags/first_tag
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   + 8ce3eae...bfc616e HEAD -> first_tag (forced update)

# Wait for the warm bookmark cache to catch up with the latest changes
  $ sleep 10 
  $ wait_for_git_bookmark_move "refs/tags/first_tag" "$current_first_tag"

# Clone the repo in a new folder
  $ cd "$TESTTMP"
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git new_repo
  $ cd new_repo

# List all the known refs. Notice that first_tag should have been moved and should now point to bfc616e
# but because of Mononoke Git bug, it is still pointing to 8963e1f
  $ git show-ref | grep tags | sort
  8963e1f55d1346a07c3aec8c8fc72bf87d0452b1 refs/tags/first_tag
  ce5a26ba55c422e8e3960224153ef5cf35e75b14 refs/tags/second_tag

# The bonsai tag mapping entries table is the culprit. It still has the old entry for the first_tag but since first_tag was converted from annotated to simple tag,
# the corresponding bonsai_tag_mapping entry should have been deleted. But it is not.
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT tag_name, hex(changeset_id) as cs_id, hex(tag_hash) as tag_hash, target_is_tag FROM bonsai_tag_mapping ORDER BY tag_name"
  tags/first_tag|5CA579C0E3EBEA708371B65CE559E5A51B231AD1B6F3CDFD874CA27362A2A6A8|8963E1F55D1346A07C3AEC8C8FC72BF87D0452B1|0
  tags/second_tag|B40C7E078B46D907C6679AA511B981242845EB3D3F7AF3719B863E8833503EFA|CE5A26BA55C422E8E3960224153EF5CF35E75B14|0

# The bookmark table correctly reflects the new state of the world with first_tag having moved from the previous commit. With bookmarks and bonsai_tag_mapping out of sync,
# we see issues in production like in S520024
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT name, hex(changeset_id) FROM bookmarks WHERE name LIKE 'tags/%' ORDER BY name"
  tags/first_tag|E70BAE430CAF70F469DD5517D77F396290A0CAD67AF436397849F01075413ED2
  tags/second_tag|DA93DC81BADD8D407DB0F3219EC0EC78F1EF750EBFA95735BB483310371AF80C
