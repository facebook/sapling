# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ MULTIPLEXED=2 setup_common_config "blob_files"
  $ cd "$TESTTMP"

Create commits using testtool drawdag
  $ testtool_drawdag -R repo --print-hg-hashes <<'EOF'
  > A-B-C
  > # bookmark: C master_bookmark
  > EOF
  A=* (glob)
  B=* (glob)
  C=* (glob)

Populate WAL queue by simulating failed writes to blobstore 0
  $ mononoke_testtool populate-wal -R repo --blobstore-path "$TESTTMP/blobstore" --source-blobstore-id 1 --target-blobstore-id 1 --delete-target-blobs
  Found 33 blobs in source blobstore 1
  Deleted 33 blobs from target blobstore 0
  Inserted 33 WAL entries for target multiplex_id 1

Check that healer queue has successful items
  $ read_blobstore_wal_queue_size
  33

Check the number of blobs.  Scrub should process every blob once.
  $ ls $TESTTMP/blobstore/1/blobs/blob-repo0000.* | grep -v .filenode_lookup. | wc -l
  30

Check that scrub doesnt report issues despite one store being missing, as the entries needed are on the queue and less than N minutes old
# TODO(mbthomas): concurrent fetches may not hit in the cache
  $ mononoke_walker --blobstore-scrub-action=ReportOnly scrub -q -I deep -b master_bookmark 2>&1 | grep -vE "(Bytes|Raw|Walked)/s" | sed -re 's/(scrub: blobstore_id BlobstoreId.0. not repaired for repo0000.).*/\1/' | uniq -c | sed 's/^ *//' | sort
  1 [INFO] Walking edge types [AliasContentMappingToFileContent, BonsaiHgMappingToHgChangesetViaBonsai, BookmarkToChangeset, ChangesetToBonsaiHgMapping, ChangesetToBonsaiParent, ChangesetToFileContent, FileContentMetadataV2ToGitSha1Alias, FileContentMetadataV2ToSeededBlake3Alias, FileContentMetadataV2ToSha1Alias, FileContentMetadataV2ToSha256Alias, FileContentToFileContentMetadataV2, HgBonsaiMappingToChangeset, HgChangesetToHgManifest, HgChangesetToHgParent, HgChangesetViaBonsaiToHgChangeset, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgBonsaiMapping, HgFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  1 [INFO] Walking node types [AliasContentMapping, BonsaiHgMapping, Bookmark, Changeset, FileContent, FileContentMetadataV2, HgBonsaiMapping, HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgFileNode, HgManifest]
  1 [INFO] [walker scrub{repo=repo}] Seen,Loaded: 25,25
  * [WARN] [walker scrub{repo=repo}] scrub: blobstore_id BlobstoreId(0) not repaired for repo0000. (glob)
