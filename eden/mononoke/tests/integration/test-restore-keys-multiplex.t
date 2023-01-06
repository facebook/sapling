# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ MULTIPLEXED=1 default_setup_blobimport "blob_files"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting

Write one blob with corrupt content
  $ CORRUPT_BLOB_KEY_DEST_REPO="$TESTTMP"/blobstore/0/blobs/blob-repo0000.hgfilenode.sha1.005d992c5dcf32993668f7cede29d296c494a5d9
  $ CORRUPT_BLOB_KEY_SRC_REPO="$TESTTMP"/blobstore/1/blobs/blob-repo0000.hgfilenode.sha1.005d992c5dcf32993668f7cede29d296c494a5d9

  $ echo a > "$CORRUPT_BLOB_KEY_DEST_REPO"
  $ sha256sum "$CORRUPT_BLOB_KEY_DEST_REPO"
  87428fc522803d31065e7bce3cf03fe475096631e5e07bbd7a0fde60c4cf25c7  $TESTTMP/blobstore/0/blobs/blob-repo0000.hgfilenode.sha1.005d992c5dcf32993668f7cede29d296c494a5d9

  $ sha256sum "$CORRUPT_BLOB_KEY_SRC_REPO"
  8fda2dd669bdf86062db431a0f04b63b7ecc8e0b56006ca257f1eade0bec82c8  $TESTTMP/blobstore/1/blobs/blob-repo0000.hgfilenode.sha1.005d992c5dcf32993668f7cede29d296c494a5d9


Check that walker fails on the corrupted blobstore
  $ mononoke_walker -L graph scrub -q --inner-blobstore-id=0 -I deep -b master_bookmark 2>&1 | strip_glog
  Execution error: Could not step to OutgoingEdge { label: HgManifestToHgFileEnvelope, target: HgFileEnvelope(HgFileNodeId(HgNodeHash(Sha1(005d992c5dcf32993668f7cede29d296c494a5d9)))), path: None } via Some(EmptyRoute) in repo repo
  
  Caused by:
      0: error while deserializing blob for 'HgFileEnvelope'
      1: end of file reached
  Error: Execution failed

Check that walker detects keys, which need to be repaired
  $ mononoke_walker --scuba-dataset file://scuba-reportonly.json -l loaded --blobstore-scrub-action=ReportOnly scrub -q -I deep -b master_bookmark 2>&1 | strip_glog | sed -re 's/^(scrub: blobstore_id BlobstoreId.0. not repaired for repo0000.).*/\1/' | uniq -c | sed 's/^ *//'
  1 Execution error: Could not step to OutgoingEdge { label: HgManifestToHgFileEnvelope, target: HgFileEnvelope(HgFileNodeId(HgNodeHash(Sha1(005d992c5dcf32993668f7cede29d296c494a5d9)))), path: None } via Some(EmptyRoute) in repo repo
  1 
  1 Caused by:
  1     Different blobstores have different values for this item: * (glob)
  1 Error: Execution failed

  $ cat > "$TESTTMP"/keys <<EOF
  > repo0000.hgfilenode.sha1.005d992c5dcf32993668f7cede29d296c494a5d9
  > EOF

Copy missing key from the healthy inner blobstore
  $ copy_blobstore_keys "$REPOID" "$REPOID" --input-file "$TESTTMP"/keys \
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
  $ mononoke_walker -L graph scrub -q --inner-blobstore-id=0 -I deep -b master_bookmark 2>&1 | strip_glog
  Seen,Loaded: 40,40
  Bytes/s,Keys/s,Bytes,Keys; Delta 000000/s,000000/s,2168,30,0s; Run 000000/s,000000/s,2168,30,0s; Type:Raw,Compressed AliasContentMapping:333,9 BonsaiHgMapping:281,3 Bookmark:0,0 Changeset:277,3 FileContent:12,3 FileContentMetadata:351,3 HgBonsaiMapping:0,0 HgChangeset:281,3 HgChangesetViaBonsai:0,0 HgFileEnvelope:189,3 HgFileNode:0,0 HgManifest:444,3
