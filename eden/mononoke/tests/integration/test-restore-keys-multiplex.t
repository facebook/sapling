# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ MULTIPLEXED=1 setup_common_config "blob_files"

  $ testtool_drawdag -R repo << EOF
  > C
  > |
  > B
  > |
  > A
  > # bookmark: C master_bookmark
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2

Write one blob with corrupt content
  $ CORRUPT_BLOB_KEY_DEST_REPO="$TESTTMP"/blobstore/0/blobs/blob-repo0000.content_metadata2.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d
  $ CORRUPT_BLOB_KEY_SRC_REPO="$TESTTMP"/blobstore/1/blobs/blob-repo0000.content_metadata2.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d

  $ echo a > "$CORRUPT_BLOB_KEY_DEST_REPO"
  $ sha256sum "$CORRUPT_BLOB_KEY_DEST_REPO"
  87428fc522803d31065e7bce3cf03fe475096631e5e07bbd7a0fde60c4cf25c7  $TESTTMP/blobstore/0/blobs/blob-repo0000.content_metadata2.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d

  $ sha256sum "$CORRUPT_BLOB_KEY_SRC_REPO"
  b94dabfcdfa52b03a0c2a7bb7728e26b248a66e491780cc1dcd21dc891ef45cd  $TESTTMP/blobstore/1/blobs/blob-repo0000.content_metadata2.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d


Check that walker fails on the corrupted blobstore
  $ mononoke_walker scrub -q --inner-blobstore-id=0 -I deep -b master_bookmark 2>&1 | grep -v 'Walking .* types'
  [ERROR] Execution error: Could not step to OutgoingEdge { label: FileContentToFileContentMetadataV2, target: FileContentMetadataV2(ContentId(Blake2(896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d))), path: None } via Some(EmptyRoute) in repo repo
  
  Caused by:
      0: error while deserializing blob for 'ContentMetadataV2'
      1: end of file reached
  Error: Execution failed




Check that walker detects keys, which need to be repaired
  $ mononoke_walker --scuba-log-file scuba-reportonly.json --blobstore-scrub-action=ReportOnly scrub -q -I deep -b master_bookmark 2>&1 | grep -v 'Walking .* types'
  [ERROR] Execution error: Could not step to OutgoingEdge { label: FileContentToFileContentMetadataV2, target: FileContentMetadataV2(ContentId(Blake2(896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d))), path: None } via Some(EmptyRoute) in repo repo
  
  Caused by:
      Different blobstores have different values for this item: * (glob)
  Error: Execution failed


  $ cat > "$TESTTMP"/keys <<EOF
  > repo0000.content_metadata2.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d
  > EOF

Copy missing key from the healthy inner blobstore
  $ mononoke_admin --blobstore-put-behaviour Overwrite blobstore copy-keys --source-repo-id "$REPOID" --target-repo-id "$REPOID" --input-file "$TESTTMP"/keys \
  > --strip-source-repo-prefix \
  > --error-keys-output "$TESTTMP"/errors \
  > --missing-keys-output "$TESTTMP"/missing \
  > --success-keys-output "$TESTTMP"/success \
  > --source-inner-blobstore-id 1 \
  > --target-inner-blobstore-id 0
  * using repo "repo" repoid RepositoryId(0) (glob)
  * using repo "repo" repoid RepositoryId(0) (glob)
  * 1 keys to copy (glob)
  * 1 keys processed (glob)
  * 1 keys were copied (glob)

Walker now should process previously corrupted blobstore correctly
# TODO(mbthomas): concurrent fetches may not hit in the cache
  $ mononoke_walker scrub -q --inner-blobstore-id=0 -I deep -b master_bookmark 2>&1 | grep -v 'Walking .* types' | grep -v 'Walked/s'
  [INFO] [walker scrub{repo=repo}] Seen,Loaded: 25,25
  [INFO] [walker scrub{repo=repo}] Bytes/s,Keys/s,Bytes,Keys; Delta 000000/s,000000/s,1225,21,0s; Run 000000/s,000000/s,1225,21,0s; Type:Raw,Compressed AliasContentMapping:444,12 BonsaiHgMapping:0,0 Bookmark:0,0 Changeset:283,3 FileContent:12,3 FileContentMetadataV2:486,3 HgBonsaiMapping:0,0 HgChangeset:0,0 HgChangesetViaBonsai:0,0 HgFileEnvelope:0,0 HgFileNode:0,0 HgManifest:0,0
