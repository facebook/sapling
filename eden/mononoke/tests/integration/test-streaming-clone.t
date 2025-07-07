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

  $ LOG_FILE="$TESTTMP/log_file"
  $ streaming_clone --scuba-log-file "$LOG_FILE" create --dot-hg-path "$TESTTMP/repo/.hg"
  * using repo "repo" repoid RepositoryId(0) (glob)
  [INFO] [streaming clone create{repo=repo}] current sizes in database: index: 0, data: 0
  [INFO] [streaming clone create{repo=repo}] about to upload 1 entries
  [INFO] [streaming clone create{repo=repo}] inserting into streaming clone database
  [INFO] [streaming clone create{repo=repo}] current max chunk num is None
  $ jq .normal.reponame < "$LOG_FILE"
  "repo"
  $ jq .normal.chunks_inserted < "$LOG_FILE"
  "1"

Try creating again, this should fail
  $ streaming_clone create --dot-hg-path "$TESTTMP/repo/.hg"
  * using repo "repo" repoid RepositoryId(0) (glob)
  * cannot create new streaming clone chunks because they already exists (glob)
  [1]

  $ start_and_wait_for_mononoke_server
  $ cd "$TESTTMP"
  $ hg clone mono:repo repo-streamclone
  Cloning repo into $TESTTMP/repo-streamclone
  fetching changelog
  2 files to transfer, 363 bytes of data
  transferred 363 bytes in * seconds (*) (glob)
  fetching selected remote bookmarks
  Checking out 'master_bookmark'
  3 files updated

  $ diff repo-streamclone/.hg/store/00changelog.i repo/.hg/store/00changelog.i
  $ diff repo-streamclone/.hg/store/00changelog.d repo/.hg/store/00changelog.d
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select idx_blob_name, data_blob_name from streaming_changelog_chunks where repo_id = 0 order by chunk_num asc;"
  streaming_clone-chunk000000-a7711e0aa3614708cef4ee3f0ea0f9eafd3473ed38f5dd54a1cf05dc81f14460270ba3928982ad7bd0dc5d767dad4527f7da9d8c5bd242ce26dc91e0296c5476-idx|streaming_clone-chunk000000-9e7ee04d9382d5fec57c9aac2c515065f50571b9ac8317a106afcbd5e18398ff894236cf450e76591710cab7dd0c293c3eeadc64a38e4f4fc3f2d433ede18bcf-data

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "delete from streaming_changelog_chunks where repo_id = 0;"
  $ streaming_clone create --dot-hg-path "$TESTTMP/repo/.hg" --max-data-chunk-size 1
  * using repo "repo" repoid RepositoryId(0) (glob)
  [INFO] [streaming clone create{repo=repo}] current sizes in database: index: 0, data: 0
  [INFO] [streaming clone create{repo=repo}] about to upload 3 entries
  [INFO] [streaming clone create{repo=repo}] inserting into streaming clone database
  [INFO] [streaming clone create{repo=repo}] current max chunk num is None
  $ rm -rf "$TESTTMP/repo-streamclone"
  $ cd "$TESTTMP"
  $ hg clone mono:repo repo-streamclone
  Cloning repo into $TESTTMP/repo-streamclone
  fetching changelog
  2 files to transfer, 363 bytes of data
  transferred 363 bytes in 0.0 seconds (354 KB/sec)
  fetching selected remote bookmarks
  Checking out 'master_bookmark'
  3 files updated
  $ diff repo-streamclone/.hg/store/00changelog.i repo/.hg/store/00changelog.i
  $ diff repo-streamclone/.hg/store/00changelog.d repo/.hg/store/00changelog.d
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select idx_blob_name, data_blob_name from streaming_changelog_chunks where repo_id = 0 order by chunk_num asc;"
  streaming_clone-chunk000000-4738b2c507af23107613b6db1c970f8cba74778df6dbdcc639e17212d5f4cccae13f635da5e5f5a0e9d7b8aa96992a37b52f30edc78f08022aba37dd049ed650-idx|streaming_clone-chunk000000-1787b46c1fd5be0ccf8ebd80c0f24a8f303c7d21e223c43c066433da96ac85cbcf4ea63534bab2cf9ca86b7fe7ec084d2b12e076459478538948f441795f7f6e-data
  streaming_clone-chunk000001-7453bedd0fe662a16f78869a5dd71f505d9e2bfcb8a16bc4e4f58140af10652dc437e602f47770ee2204920e851e26c61ccd467af37f9420527fdef2b933893b-idx|streaming_clone-chunk000001-1501cfc24716f7dc7f0436959a56f94d71f91077f4319e2426044095fd367f4c42f48a8ef3624160188e0a57faa85ee27d642d39e2c2dd094f01976122f58d44-data
  streaming_clone-chunk000002-ae9d697d84f07eef1c9d0ad1900e094f4670fedcb9ed053359ec57d1957864fe8977db424ff64f98b5902f9b328d225e122c62f9db856bdabaaeb6788c2167f3-idx|streaming_clone-chunk000002-c1b7498ffa9d9a5855b3824dde3d956fc988075a514abeaa0146643f4b27bfcaf0473bf296ee71aecf1d9ca2ea3cf1ea035bcad15fb6342f7fb90273e0b2324d-data

Push a few new commits and update streaming clone
  $ cd "$TESTTMP"
  $ hg clone -q mono:repo repo-push --noupdate
  $ cd repo-push
  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 1 > 1
  $ hg add 1
  $ hg ci -m 'add 1'
  $ hg push -r . --to master_bookmark
  pushing rev db986f33ca52 to destination mono:repo bookmark master_bookmark
  searching for changes
  updating bookmark master_bookmark
  $ echo 11 > 1
  $ hg ci -m 'echo 11'
  $ hg push -r . --to master_bookmark
  pushing rev 616ef3a47d98 to destination mono:repo bookmark master_bookmark
  searching for changes
  updating bookmark master_bookmark
  $ echo 111 > 1
  $ hg ci -m 'echo 111'
  $ hg push -r . --to master_bookmark
  pushing rev 4583ead66637 to destination mono:repo bookmark master_bookmark
  searching for changes
  updating bookmark master_bookmark
  $ hg log -r tip
  commit:      4583ead66637
  bookmark:    remote/master_bookmark
  hoistedname: master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     echo 111
  


  $ cd "$TESTTMP"
  $ hg clone mono:repo repo-streamclone-2
  Cloning repo into $TESTTMP/repo-streamclone-2
  fetching changelog
  2 files to transfer, 363 bytes of data
  transferred 363 bytes in * seconds (* KB/sec) (glob)
  fetching selected remote bookmarks
  Checking out 'master_bookmark'
  4 files updated

Check that with last chunk skipping no new batches are uploaded
  $ streaming_clone update --dot-hg-path "$TESTTMP/repo-streamclone-2/.hg" --skip-last-chunk
  * using repo "repo" repoid RepositoryId(0) (glob)
  [INFO] [streaming clone update{repo=repo}] current sizes in database: index: 192, data: 171
  [INFO] [streaming clone update{repo=repo}] about to upload 0 entries
  [INFO] [streaming clone update{repo=repo}] inserting into streaming clone database
  [INFO] [streaming clone update{repo=repo}] current max chunk num is Some(2)

  $ streaming_clone update --dot-hg-path "$TESTTMP/repo-streamclone-2/.hg"
  * using repo "repo" repoid RepositoryId(0) (glob)
  [INFO] [streaming clone update{repo=repo}] current sizes in database: index: 192, data: 171
  [INFO] [streaming clone update{repo=repo}] about to upload 1 entries
  [INFO] [streaming clone update{repo=repo}] inserting into streaming clone database
  [INFO] [streaming clone update{repo=repo}] current max chunk num is Some(2)

Clone it again to make sure saved streaming chunks are valid
  $ cd "$TESTTMP"
  $ hg clone mono:repo repo-streamclone-3
  Cloning repo into $TESTTMP/repo-streamclone-3
  fetching changelog
  2 files to transfer, 737 bytes of data
  transferred 737 bytes in 0.0 seconds (* KB/sec) (glob)
  fetching selected remote bookmarks
  Checking out 'master_bookmark'
  4 files updated
  $ cd repo-streamclone-3
  $ hg log -r tip
  commit:      4583ead66637
  bookmark:    remote/master_bookmark
  hoistedname: master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     echo 111
  


  $ cd "$TESTTMP"
  $ diff repo-streamclone-2/.hg/store/00changelog.i repo-streamclone-3/.hg/store/00changelog.i
  $ diff repo-streamclone-2/.hg/store/00changelog.d repo-streamclone-3/.hg/store/00changelog.d

Check no-upload-if-less-than-chunks option
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "delete from streaming_changelog_chunks where repo_id = 0;"
  $ streaming_clone create --dot-hg-path "$TESTTMP/repo/.hg" --no-upload-if-less-than-chunks 2
  * using repo "repo" repoid RepositoryId(0) (glob)
  [INFO] [streaming clone create{repo=repo}] current sizes in database: index: 0, data: 0
  [INFO] [streaming clone create{repo=repo}] has too few chunks to upload - 1. Exiting
  $ streaming_clone create --dot-hg-path "$TESTTMP/repo/.hg" --no-upload-if-less-than-chunks 2 --max-data-chunk-size 1
  * using repo "repo" repoid RepositoryId(0) (glob)
  [INFO] [streaming clone create{repo=repo}] current sizes in database: index: 0, data: 0
  [INFO] [streaming clone create{repo=repo}] about to upload 3 entries
  [INFO] [streaming clone create{repo=repo}] inserting into streaming clone database
  [INFO] [streaming clone create{repo=repo}] current max chunk num is None
