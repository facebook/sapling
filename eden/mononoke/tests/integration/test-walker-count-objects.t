# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ default_setup_pre_blobimport "blob_files"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  |
  o  B [draft;rev=1;112478962961]
  |
  o  A [draft;rev=0;426bada5c675]
  $
  $ blobimport repo-hg/.hg repo --derived-data-type=fsnodes

check blobstore numbers, walk will do some more steps for mappings
  $ BLOBPREFIX="$TESTTMP/blobstore/blobs/blob-repo0000"
  $ BONSAICOUNT=$(ls $BLOBPREFIX.changeset.* $BLOBPREFIX.content.* $BLOBPREFIX.content_metadata.* | count_stdin_lines)
  $ echo "$BONSAICOUNT"
  9
  $ HGCOUNT=$(ls $BLOBPREFIX.* | grep -E '.(filenode_lookup|hgchangeset|hgfilenode|hgmanifest).' | count_stdin_lines)
  $ echo "$HGCOUNT"
  12
  $ BLOBCOUNT=$(ls $BLOBPREFIX.* | grep -v .alias. | count_stdin_lines)
  $ echo "$BLOBCOUNT"
  27

count-objects, bonsai core data.  total nodes is BONSAICOUNT plus one for the root bookmark step.
  $ mononoke_walker --storage-id=blobstore --readonly-storage scrub -q --bookmark master_bookmark -I bonsai 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [BonsaiChangesetToBonsaiParent, BonsaiChangesetToFileContent, BookmarkToBonsaiChangeset]
  Walking node types [BonsaiChangeset, Bookmark, FileContent]
  Final count: (7, 7)
  Bytes/s,* (glob)
  * Type:Walked,Checks,Children BonsaiChangeset:3,* Bookmark:1,1,1 FileContent:3,3,0 (glob)

count-objects, shallow, bonsai only.  No parents, expect just one of each node type
  $ mononoke_walker --storage-id=blobstore --readonly-storage scrub -q --bookmark master_bookmark -I shallow -X hg -x BonsaiHgMapping 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [AliasContentMappingToFileContent, BonsaiChangesetToBonsaiFsnodeMapping, BonsaiChangesetToFileContent, BonsaiToRootFsnode, BookmarkToBonsaiChangeset, FileContentMetadataToGitSha1Alias, FileContentMetadataToSha1Alias, FileContentMetadataToSha256Alias, FileContentToFileContentMetadata, FsnodeToChildFsnode]
  Walking node types [AliasContentMapping, BonsaiChangeset, BonsaiFsnodeMapping, Bookmark, FileContent, FileContentMetadata, Fsnode]
  Final count: (9, 9)
  Bytes/s,* (glob)
  * Type:Walked,Checks,Children AliasContentMapping:3,3,0 BonsaiChangeset:1,1,3 BonsaiFsnodeMapping:1,1,1 Bookmark:1,1,1 FileContent:1,1,0 FileContentMetadata:1,0,3 Fsnode:1,1,0 (glob)

count-objects, hg only. total nodes is HGCOUNT plus 1 for the root bookmark step, plus 1 for mapping from bookmark to hg. plus 3 for filenode (same blob as envelope)
  $ mononoke_walker --storage-id=blobstore --readonly-storage scrub -q --bookmark master_bookmark -I hg 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [BonsaiHgMappingToHgChangeset, BookmarkToBonsaiHgMapping, HgChangesetToHgManifest, HgChangesetToHgParent, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgLinkNodeToHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  Walking node types [BonsaiHgMapping, Bookmark, FileContent, HgChangeset, HgFileEnvelope, HgFileNode, HgManifest]
  Final count: (17, 17)
  Bytes/s,* (glob)
  * Type:Walked,Checks,Children BonsaiHgMapping:1,1,1 Bookmark:1,1,1 FileContent:3,3,0 HgChangeset:3,*,5 HgFileEnvelope:3,*,3 HgFileNode:3,*,0 HgManifest:3,3,6 (glob)

count-objects, default deep walk across bonsai and hg data.  BLOBCOUNT plus mappings and root.
  $ mononoke_walker --storage-id=blobstore --readonly-storage scrub -q --bookmark master_bookmark -I deep 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [AliasContentMappingToFileContent, BonsaiChangesetToBonsaiFsnodeMapping, BonsaiChangesetToBonsaiHgMapping, BonsaiChangesetToBonsaiParent, BonsaiChangesetToFileContent, BonsaiHgMappingToHgChangeset, BonsaiToRootFsnode, BookmarkToBonsaiChangeset, FileContentMetadataToGitSha1Alias, FileContentMetadataToSha1Alias, FileContentMetadataToSha256Alias, FileContentToFileContentMetadata, FsnodeToChildFsnode, HgBonsaiMappingToBonsaiChangeset, HgChangesetToHgManifest, HgChangesetToHgParent, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgLinkNodeToHgBonsaiMapping, HgLinkNodeToHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  Walking node types [AliasContentMapping, BonsaiChangeset, BonsaiFsnodeMapping, BonsaiHgMapping, Bookmark, FileContent, FileContentMetadata, Fsnode, HgBonsaiMapping, HgChangeset, HgFileEnvelope, HgFileNode, HgManifest]
  Final count: (43, 43)
  Bytes/s,* (glob)
  * Type:Walked,Checks,Children AliasContentMapping:9,9,0 BonsaiChangeset:3,3,14 BonsaiFsnodeMapping:3,3,3 BonsaiHgMapping:3,* Bookmark:1,1,1 FileContent:3,*,0 FileContentMetadata:3,0,9 Fsnode:3,3,0 HgBonsaiMapping:3,3,0 HgChangeset:3,* HgFileEnvelope:3,*,0 HgFileNode:3,*,3 HgManifest:3,3,6 (glob)

count-objects, default shallow walk across bonsai and hg data
  $ mononoke_walker --storage-id=blobstore --readonly-storage scrub -q --bookmark master_bookmark -I shallow 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [AliasContentMappingToFileContent, BonsaiChangesetToBonsaiFsnodeMapping, BonsaiChangesetToBonsaiHgMapping, BonsaiChangesetToFileContent, BonsaiHgMappingToHgChangeset, BonsaiToRootFsnode, BookmarkToBonsaiChangeset, FileContentMetadataToGitSha1Alias, FileContentMetadataToSha1Alias, FileContentMetadataToSha256Alias, FileContentToFileContentMetadata, FsnodeToChildFsnode, HgChangesetToHgManifest, HgFileEnvelopeToFileContent, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  Walking node types [AliasContentMapping, BonsaiChangeset, BonsaiFsnodeMapping, BonsaiHgMapping, Bookmark, FileContent, FileContentMetadata, Fsnode, HgChangeset, HgFileEnvelope, HgFileNode, HgManifest]
  Final count: (28, 28)
  Bytes/s,* (glob)
  * Type:Walked,Checks,Children AliasContentMapping:9,9,0 BonsaiChangeset:1,1,4 BonsaiFsnodeMapping:1,1,1 BonsaiHgMapping:1,1,1 Bookmark:1,1,1 FileContent:3,*,0 FileContentMetadata:3,0,9 Fsnode:1,1,0 HgChangeset:1,1,1 HgFileEnvelope:3,*,4 HgFileNode:3,3,0 HgManifest:1,1,6 (glob)

count-objects, default shallow walk across bonsai and hg data, including mutable
  $ mononoke_walker --storage-id=blobstore --readonly-storage scrub -q --bookmark master_bookmark -I shallow -I marker 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [AliasContentMappingToFileContent, BonsaiChangesetToBonsaiFsnodeMapping, BonsaiChangesetToBonsaiHgMapping, BonsaiChangesetToBonsaiPhaseMapping, BonsaiChangesetToFileContent, BonsaiHgMappingToHgChangeset, BonsaiToRootFsnode, BookmarkToBonsaiChangeset, FileContentMetadataToGitSha1Alias, FileContentMetadataToSha1Alias, FileContentMetadataToSha256Alias, FileContentToFileContentMetadata, FsnodeToChildFsnode, HgChangesetToHgManifest, HgFileEnvelopeToFileContent, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  Walking node types [AliasContentMapping, BonsaiChangeset, BonsaiFsnodeMapping, BonsaiHgMapping, BonsaiPhaseMapping, Bookmark, FileContent, FileContentMetadata, Fsnode, HgChangeset, HgFileEnvelope, HgFileNode, HgManifest]
  Final count: (29, 29)
  Bytes/s,* (glob)
  * Type:Walked,Checks,Children AliasContentMapping:9,9,0 BonsaiChangeset:1,1,5 BonsaiFsnodeMapping:1,1,1 BonsaiHgMapping:1,1,1 BonsaiPhaseMapping:1,1,0 Bookmark:1,1,1 FileContent:3,7,0 FileContentMetadata:3,0,9 Fsnode:1,1,0 HgChangeset:1,1,1 HgFileEnvelope:3,3,4 HgFileNode:3,3,0 HgManifest:1,1,6 (glob)

count-objects, default shallow walk across bonsai and hg data, including mutable for all public heads
  $ mononoke_walker --storage-id=blobstore --readonly-storage scrub -q --walk-root PublishedBookmarks -I shallow -I marker 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [AliasContentMappingToFileContent, BonsaiChangesetToBonsaiFsnodeMapping, BonsaiChangesetToBonsaiHgMapping, BonsaiChangesetToBonsaiPhaseMapping, BonsaiChangesetToFileContent, BonsaiHgMappingToHgChangeset, BonsaiToRootFsnode, FileContentMetadataToGitSha1Alias, FileContentMetadataToSha1Alias, FileContentMetadataToSha256Alias, FileContentToFileContentMetadata, FsnodeToChildFsnode, HgChangesetToHgManifest, HgFileEnvelopeToFileContent, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode, PublishedBookmarksToBonsaiChangeset, PublishedBookmarksToBonsaiHgMapping]
  Walking node types [AliasContentMapping, BonsaiChangeset, BonsaiFsnodeMapping, BonsaiHgMapping, BonsaiPhaseMapping, FileContent, FileContentMetadata, Fsnode, HgChangeset, HgFileEnvelope, HgFileNode, HgManifest, PublishedBookmarks]
  Final count: (30, 30)
  Bytes/s,* (glob)
  * Type:Walked,Checks,Children AliasContentMapping:9,9,0 BonsaiChangeset:1,1,5 BonsaiFsnodeMapping:1,1,1 BonsaiHgMapping:2,2,1 BonsaiPhaseMapping:1,1,0 FileContent:3,*,0 FileContentMetadata:3,0,9 Fsnode:1,1,0 HgChangeset:1,*,1 HgFileEnvelope:3,*,4 HgFileNode:3,3,0 HgManifest:1,1,6 PublishedBookmarks:1,1,2 (glob)
