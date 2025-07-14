# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ MULTIPLEXED=2 default_setup_blobimport "blob_files"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting

Erase the sqllites and blobstore_sync_queue
  $ rm -rf "$TESTTMP/monsql/sqlite_dbs" "$TESTTMP/blobstore_sync_queue/sqlite_dbs" "$TESTTMP/blobstore"

blobimport them into Mononoke storage again, but with write failures on one side
  $ blobimport repo/.hg repo --blobstore-write-chaos-rate=1

Check that healer queue has successful items
  $ read_blobstore_wal_queue_size
  33

Check the number of blobs.  Scrub should process every blob once.
  $ ls $TESTTMP/blobstore/1/blobs/blob-repo0000.* | grep -v .filenode_lookup. | wc -l
  30

Check that scrub doesnt report issues despite one store being missing, as the entries needed are on the queue and less than N minutes old
# TODO(mbthomas): concurrent fetches may not hit in the cache
  $ mononoke_walker -l loaded --blobstore-scrub-action=ReportOnly scrub -q -I deep -b master_bookmark 2>&1 | grep -vE "(Bytes|Raw|Walked)/s" | sed -re 's/(scrub: blobstore_id BlobstoreId.0. not repaired for repo0000.).*/\1/' | uniq -c | sed 's/^ *//' | sort
  1 [INFO] Walking edge types [AliasContentMappingToFileContent, BonsaiHgMappingToHgChangesetViaBonsai, BookmarkToChangeset, ChangesetToBonsaiHgMapping, ChangesetToBonsaiParent, ChangesetToFileContent, FileContentMetadataV2ToGitSha1Alias, FileContentMetadataV2ToSeededBlake3Alias, FileContentMetadataV2ToSha1Alias, FileContentMetadataV2ToSha256Alias, FileContentToFileContentMetadataV2, HgBonsaiMappingToChangeset, HgChangesetToHgManifest, HgChangesetToHgParent, HgChangesetViaBonsaiToHgChangeset, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgBonsaiMapping, HgFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  1 [INFO] Walking node types [AliasContentMapping, BonsaiHgMapping, Bookmark, Changeset, FileContent, FileContentMetadataV2, HgBonsaiMapping, HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgFileNode, HgManifest]
  1 [INFO] [walker scrub{repo=repo}] Seen,Loaded: 43,43
  * [WARN] [walker scrub{repo=repo}] scrub: blobstore_id BlobstoreId(0) not repaired for repo0000. (glob)
