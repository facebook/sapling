# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig format.use-segmented-changelog=false

# This is the "modern" way to trigger a streaming clone (only streams changelog - not files).
  $ setconfig clone.use-rust=true clone.use-commit-graph=false

setup configuration
  $ BLOB_TYPE="blob_files" default_setup_drawdag
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2

Try creating with a tag
  $ TAG=another_mainline
  $ streaming_clone create --dot-hg-path "$TESTTMP/repo/.hg" --tag another_mainline
  * using repo "repo" repoid RepositoryId(0) (glob)
  [INFO] [streaming clone create{repo=repo tag="another_mainline"}] current sizes in database: index: 0, data: 0
  [INFO] [streaming clone create{repo=repo tag="another_mainline"}] about to upload 1 entries
  [INFO] [streaming clone create{repo=repo tag="another_mainline"}] inserting into streaming clone database
  [INFO] [streaming clone create{repo=repo tag="another_mainline"}] current max chunk num is None


  $ start_and_wait_for_mononoke_server
Clone - check that no bytes were transferred from streaming clone because no tags were used
  $ cd $TESTTMP
  $ hg clone mono:repo repo-streamclone
  Cloning repo into $TESTTMP/repo-streamclone
  fetching changelog
  2 files to transfer, 0 bytes of data (glob)
  transferred 0 bytes in * seconds (*) (glob)
  fetching selected remote bookmarks
  Checking out 'master_bookmark'
  3 files updated

  $ diff repo-streamclone/.hg/store/00changelog.i repo/.hg/store/00changelog.i
  $ diff repo-streamclone/.hg/store/00changelog.d repo/.hg/store/00changelog.d
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select tag, idx_blob_name, data_blob_name from streaming_changelog_chunks where repo_id = 0 order by tag, chunk_num asc;"
  another_mainline|streaming_clone-chunk000000-a7711e0aa3614708cef4ee3f0ea0f9eafd3473ed38f5dd54a1cf05dc81f14460270ba3928982ad7bd0dc5d767dad4527f7da9d8c5bd242ce26dc91e0296c5476-idx|streaming_clone-chunk000000-9e7ee04d9382d5fec57c9aac2c515065f50571b9ac8317a106afcbd5e18398ff894236cf450e76591710cab7dd0c293c3eeadc64a38e4f4fc3f2d433ede18bcf-data

Now clone with tag, make sure that streaming clone was used
  $ hg clone mono:repo repo-streamclone-tag --config stream_out_shallow.tag="$TAG"
  Cloning repo into $TESTTMP/repo-streamclone-tag
  fetching changelog
  2 files to transfer, 363 bytes of data
  transferred 363 bytes in * seconds (*) (glob)
  fetching selected remote bookmarks
  Checking out 'master_bookmark'
  3 files updated
