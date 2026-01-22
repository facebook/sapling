# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Test the SLAPI/EdenAPI streaming clone endpoint
# This tests the same functionality as test-streaming-clone.t but uses the
# experimental.use-slapi-streaming-clone config to route through the SLAPI endpoint.

#require slow

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig format.use-segmented-changelog=false

# Use revlog changelog for streaming clone
  $ setconfig clone.use-rust=true clone.use-commit-graph=false

setup configuration
  $ BLOB_TYPE="blob_files" default_setup_drawdag
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2

Create streaming clone data in the database
  $ streaming_clone create --dot-hg-path "$TESTTMP/repo/.hg"
  * using repo "repo" repoid RepositoryId(0) (glob)
  [INFO] [streaming clone create{repo=repo}] current sizes in database: index: 0, data: 0
  [INFO] [streaming clone create{repo=repo}] about to upload 1 entries
  [INFO] [streaming clone create{repo=repo}] inserting into streaming clone database
  [INFO] [streaming clone create{repo=repo}] current max chunk num is None

  $ start_and_wait_for_mononoke_server

Test clone using SLAPI streaming clone endpoint (instead of wireproto)
  $ cd "$TESTTMP"
  $ hg clone mono:repo repo-slapi-streamclone --config experimental.use-slapi-streaming-clone=true
  Cloning repo into $TESTTMP/repo-slapi-streamclone
  streaming changelog via SLAPI
  transferred * in * seconds (*) (glob)
  pulling from mono:repo
  Checking out 'master_bookmark'
  3 files updated

Verify the changelog files match the source
  $ diff repo-slapi-streamclone/.hg/store/00changelog.i repo/.hg/store/00changelog.i
  $ diff repo-slapi-streamclone/.hg/store/00changelog.d repo/.hg/store/00changelog.d

Verify the repository is functional - dump the whole commit graph
  $ cd repo-slapi-streamclone
  $ hg log -G -T '{node|short} {desc}\n'
  @  * C (glob)
  │
  o  * B (glob)
  │
  o  * A (glob)
  



Test clone using wireproto path (use-slapi-streaming-clone=false)
  $ cd "$TESTTMP"
  $ hg clone mono:repo repo-wireproto-streamclone --config experimental.use-slapi-streaming-clone=false
  Cloning repo into $TESTTMP/repo-wireproto-streamclone
  fetching changelog
  2 files to transfer, * bytes of data (glob)
  transferred * bytes in * seconds (*) (glob)
  fetching selected remote bookmarks
  Checking out 'master_bookmark'
  3 files updated

Verify the wireproto clone changelog files match the source
  $ diff repo-wireproto-streamclone/.hg/store/00changelog.i repo/.hg/store/00changelog.i
  $ diff repo-wireproto-streamclone/.hg/store/00changelog.d repo/.hg/store/00changelog.d

Compare both clones - SLAPI and wireproto should produce identical results
  $ diff repo-slapi-streamclone/.hg/store/00changelog.i repo-wireproto-streamclone/.hg/store/00changelog.i
  $ diff repo-slapi-streamclone/.hg/store/00changelog.d repo-wireproto-streamclone/.hg/store/00changelog.d

Verify the wireproto clone is functional
  $ cd repo-wireproto-streamclone
  $ hg log -G -T '{node|short} {desc}\n'
  @  * C (glob)
  │
  o  * B (glob)
  │
  o  * A (glob)
  



Test clone with tag using SLAPI
  $ cd "$TESTTMP"

Delete existing chunks and recreate with tag
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "delete from streaming_changelog_chunks where repo_id = 0;"
  $ streaming_clone create --dot-hg-path "$TESTTMP/repo/.hg" --tag my_tag
  * using repo "repo" repoid RepositoryId(0) (glob)
  [INFO] [streaming clone create{repo=repo tag="my_tag"}] current sizes in database: index: 0, data: 0
  [INFO] [streaming clone create{repo=repo tag="my_tag"}] about to upload 1 entries
  [INFO] [streaming clone create{repo=repo tag="my_tag"}] inserting into streaming clone database
  [INFO] [streaming clone create{repo=repo tag="my_tag"}] current max chunk num is None

Clone with tag using SLAPI - should use streaming clone with the tag
  $ hg clone mono:repo repo-slapi-tag --config experimental.use-slapi-streaming-clone=true --config stream_out_shallow.tag=my_tag
  Cloning repo into $TESTTMP/repo-slapi-tag
  streaming changelog via SLAPI
  transferred * in * seconds (*) (glob)
  pulling from mono:repo
  Checking out 'master_bookmark'
  3 files updated

Clone without tag using SLAPI - should get 0 bytes (no default tag chunks)
  $ hg clone mono:repo repo-slapi-notag --config experimental.use-slapi-streaming-clone=true
  Cloning repo into $TESTTMP/repo-slapi-notag
  streaming changelog via SLAPI
  transferred * in * seconds (*) (glob)
  pulling from mono:repo
  fetching revlog data for 3 commits
  Checking out 'master_bookmark'
  3 files updated

Verify both clones have identical changelog files as source
  $ diff repo-slapi-tag/.hg/store/00changelog.i repo/.hg/store/00changelog.i
  $ diff repo-slapi-tag/.hg/store/00changelog.d repo/.hg/store/00changelog.d

Test clone with tag using wireproto (resembles test-streaming-clone-tag.t)
Clone with tag using wireproto - should use streaming clone with the tag
  $ hg clone mono:repo repo-wireproto-tag --config experimental.use-slapi-streaming-clone=false --config stream_out_shallow.tag=my_tag
  Cloning repo into $TESTTMP/repo-wireproto-tag
  fetching changelog
  2 files to transfer, * bytes of data (glob)
  transferred * bytes in * seconds (*) (glob)
  fetching selected remote bookmarks
  Checking out 'master_bookmark'
  3 files updated

Clone without tag using wireproto - should get 0 bytes (no default tag chunks)
  $ hg clone mono:repo repo-wireproto-notag --config experimental.use-slapi-streaming-clone=false
  Cloning repo into $TESTTMP/repo-wireproto-notag
  fetching changelog
  2 files to transfer, 0 bytes of data
  transferred 0 bytes in * seconds (*) (glob)
  fetching selected remote bookmarks
  Checking out 'master_bookmark'
  3 files updated

Verify wireproto tag clone has identical changelog files as source
  $ diff repo-wireproto-tag/.hg/store/00changelog.i repo/.hg/store/00changelog.i
  $ diff repo-wireproto-tag/.hg/store/00changelog.d repo/.hg/store/00changelog.d

Compare SLAPI and wireproto tag clones - should produce identical results
  $ diff repo-slapi-tag/.hg/store/00changelog.i repo-wireproto-tag/.hg/store/00changelog.i
  $ diff repo-slapi-tag/.hg/store/00changelog.d repo-wireproto-tag/.hg/store/00changelog.d

Verify wireproto tag clone is functional
  $ cd repo-wireproto-tag
  $ hg log -G -T '{node|short} {desc}\n'
  @  * C (glob)
  │
  o  * B (glob)
  │
  o  * A (glob)
  
  $ cd "$TESTTMP"

Test with multiple chunks
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "delete from streaming_changelog_chunks where repo_id = 0;"
  $ streaming_clone create --dot-hg-path "$TESTTMP/repo/.hg" --max-data-chunk-size 1
  * using repo "repo" repoid RepositoryId(0) (glob)
  [INFO] [streaming clone create{repo=repo}] current sizes in database: index: 0, data: 0
  [INFO] [streaming clone create{repo=repo}] about to upload 3 entries
  [INFO] [streaming clone create{repo=repo}] inserting into streaming clone database
  [INFO] [streaming clone create{repo=repo}] current max chunk num is None

  $ rm -rf "$TESTTMP/repo-slapi-multichunk"
  $ hg clone mono:repo repo-slapi-multichunk --config experimental.use-slapi-streaming-clone=true
  Cloning repo into $TESTTMP/repo-slapi-multichunk
  streaming changelog via SLAPI
  transferred * in * seconds (*) (glob)
  pulling from mono:repo
  Checking out 'master_bookmark'
  3 files updated

Verify multi-chunk clone matches the source
  $ diff repo-slapi-multichunk/.hg/store/00changelog.i repo/.hg/store/00changelog.i
  $ diff repo-slapi-multichunk/.hg/store/00changelog.d repo/.hg/store/00changelog.d
