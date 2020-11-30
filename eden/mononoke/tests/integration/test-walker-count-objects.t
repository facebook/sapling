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
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  $ blobimport repo-hg/.hg repo --derived-data-type=blame --derived-data-type=changeset_info --derived-data-type=deleted_manifest --derived-data-type=fastlog --derived-data-type=fsnodes --derived-data-type=skeleton_manifests --derived-data-type=unodes

check blobstore numbers, walk will do some more steps for mappings
  $ BLOBPREFIX="$TESTTMP/blobstore/blobs/blob-repo0000"
  $ BONSAICOUNT=$(ls $BLOBPREFIX.changeset.* $BLOBPREFIX.content.* $BLOBPREFIX.content_metadata.* | wc -l)
  $ echo "$BONSAICOUNT"
  9
  $ HGCOUNT=$(ls $BLOBPREFIX.* | grep -E '.(filenode_lookup|hgchangeset|hgfilenode|hgmanifest).' | wc -l)
  $ echo "$HGCOUNT"
  12
  $ BLOBCOUNT=$(ls $BLOBPREFIX.* | grep -v .alias. | wc -l)
  $ echo "$BLOBCOUNT"
  64

count-objects, bonsai core data.  total nodes is BONSAICOUNT plus one for the root bookmark step.
  $ mononoke_walker --readonly-storage scrub -q --bookmark master_bookmark -I bonsai 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [BookmarkToChangeset, ChangesetToBonsaiParent, ChangesetToFileContent]
  Walking node types [Bookmark, Changeset, FileContent]
  Final count: (7, 7)
  Bytes/s,* (glob)
  * Type:Walked,Checks,Children Bookmark:1,1,1 Changeset:3,* FileContent:3,3,0 (glob)

count-objects, shallow, bonsai only.  No parents, expect just one of each node type. Also exclude FsnodeToFileContent to keep the test intact
  $ mononoke_walker --readonly-storage scrub -q --bookmark master_bookmark -I shallow -X hg -x BonsaiHgMapping -X FsnodeToFileContent 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [AliasContentMappingToFileContent, BookmarkToChangeset, ChangesetToFileContent, ChangesetToFsnodeMapping, FileContentMetadataToGitSha1Alias, FileContentMetadataToSha1Alias, FileContentMetadataToSha256Alias, FileContentToFileContentMetadata, FsnodeMappingToRootFsnode, FsnodeToChildFsnode]
  Walking node types [AliasContentMapping, Bookmark, Changeset, FileContent, FileContentMetadata, Fsnode, FsnodeMapping]
  Final count: (9, 9)
  Bytes/s,* (glob)
  * Type:Walked,Checks,Children AliasContentMapping:3,3,0 Bookmark:1,1,1 Changeset:1,1,3 FileContent:1,1,0 FileContentMetadata:1,0,3 Fsnode:1,1,0 FsnodeMapping:1,1,1 (glob)

count-objects, hg only. total nodes is HGCOUNT plus 1 for the root bookmark step, plus 1 for mapping from bookmark to hg. plus 3 for filenode (same blob as envelope)
  $ mononoke_walker --readonly-storage scrub -q --bookmark master_bookmark -I hg 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [BonsaiHgMappingToHgChangeset, BookmarkToBonsaiHgMapping, HgChangesetToHgManifest, HgChangesetToHgParent, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  Walking node types [BonsaiHgMapping, Bookmark, FileContent, HgChangeset, HgFileEnvelope, HgFileNode, HgManifest]
  Final count: (17, 17)
  Bytes/s,* (glob)
  * Type:Walked,Checks,Children BonsaiHgMapping:1,1,1 Bookmark:1,1,1 FileContent:3,3,0 HgChangeset:3,*,5 HgFileEnvelope:3,*,3 HgFileNode:3,*,0 HgManifest:3,3,6 (glob)

count-objects, default shallow walk across bonsai and hg data, but exclude HgFileEnvelope so that we can test that we visit FileContent from fsnodes
  $ mononoke_walker --readonly-storage scrub -q --bookmark master_bookmark -I shallow -x HgFileEnvelope 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [AliasContentMappingToFileContent, BonsaiHgMappingToHgChangeset, BookmarkToChangeset, ChangesetToBonsaiHgMapping, ChangesetToFileContent, ChangesetToFsnodeMapping, FileContentMetadataToGitSha1Alias, FileContentMetadataToSha1Alias, FileContentMetadataToSha256Alias, FileContentToFileContentMetadata, FsnodeMappingToRootFsnode, FsnodeToChildFsnode, FsnodeToFileContent, HgChangesetToHgManifest, HgManifestToChildHgManifest, HgManifestToHgFileNode]
  Walking node types [AliasContentMapping, BonsaiHgMapping, Bookmark, Changeset, FileContent, FileContentMetadata, Fsnode, FsnodeMapping, HgChangeset, HgFileNode, HgManifest]
  Final count: (25, 25)
  Bytes/s,* (glob)
  * Type:Walked,Checks,Children AliasContentMapping:9,9,0 BonsaiHgMapping:1,1,1 Bookmark:1,1,1 Changeset:1,1,4 FileContent:3,*,0 FileContentMetadata:3,0,9 Fsnode:1,1,4 FsnodeMapping:1,1,1 HgChangeset:1,1,1 HgFileNode:3,3,* HgManifest:1,1,3 (glob)

count-objects, default shallow walk across bonsai and hg data, including mutable
  $ mononoke_walker --readonly-storage scrub -q --bookmark master_bookmark -I shallow -I marker 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [AliasContentMappingToFileContent, BonsaiHgMappingToHgChangeset, BookmarkToChangeset, ChangesetToBonsaiHgMapping, ChangesetToFileContent, ChangesetToFsnodeMapping, ChangesetToPhaseMapping, FileContentMetadataToGitSha1Alias, FileContentMetadataToSha1Alias, FileContentMetadataToSha256Alias, FileContentToFileContentMetadata, FsnodeMappingToRootFsnode, FsnodeToChildFsnode, FsnodeToFileContent, HgChangesetToHgManifest, HgFileEnvelopeToFileContent, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  Walking node types [AliasContentMapping, BonsaiHgMapping, Bookmark, Changeset, FileContent, FileContentMetadata, Fsnode, FsnodeMapping, HgChangeset, HgFileEnvelope, HgFileNode, HgManifest, PhaseMapping]
  Final count: (29, 29)
  Bytes/s,* (glob)
  * Type:Walked,Checks,Children AliasContentMapping:9,9,0 BonsaiHgMapping:1,1,1 Bookmark:1,1,1 Changeset:1,1,5 FileContent:3,*,0 FileContentMetadata:3,0,9 Fsnode:1,1,* FsnodeMapping:1,1,1 HgChangeset:1,1,1 HgFileEnvelope:3,3,* HgFileNode:3,3,0 HgManifest:1,1,6 PhaseMapping:1,1,0 (glob)

count-objects, default shallow walk across bonsai and hg data, including mutable for all public heads
  $ mononoke_walker --readonly-storage scrub -q --walk-root PublishedBookmarks -I shallow -I marker 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [AliasContentMappingToFileContent, BonsaiHgMappingToHgChangeset, ChangesetToBonsaiHgMapping, ChangesetToFileContent, ChangesetToFsnodeMapping, ChangesetToPhaseMapping, FileContentMetadataToGitSha1Alias, FileContentMetadataToSha1Alias, FileContentMetadataToSha256Alias, FileContentToFileContentMetadata, FsnodeMappingToRootFsnode, FsnodeToChildFsnode, FsnodeToFileContent, HgChangesetToHgManifest, HgFileEnvelopeToFileContent, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode, PublishedBookmarksToBonsaiHgMapping, PublishedBookmarksToChangeset]
  Walking node types [AliasContentMapping, BonsaiHgMapping, Changeset, FileContent, FileContentMetadata, Fsnode, FsnodeMapping, HgChangeset, HgFileEnvelope, HgFileNode, HgManifest, PhaseMapping, PublishedBookmarks]
  Final count: (30, 30)
  Bytes/s,* (glob)
  * Type:Walked,Checks,Children AliasContentMapping:9,9,0 BonsaiHgMapping:2,2,1 Changeset:1,1,5 FileContent:3,*,0 FileContentMetadata:3,0,9 Fsnode:1,1,* FsnodeMapping:1,1,1 HgChangeset:1,*,1 HgFileEnvelope:3,*,* HgFileNode:3,3,0 HgManifest:1,1,6 PhaseMapping:1,1,0 PublishedBookmarks:1,1,2 (glob)

count-objects, shallow walk across bonsai and changeset_info
  $ mononoke_walker --readonly-storage scrub -q --bookmark master_bookmark -I shallow -i bonsai -i derived_changeset_info 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [BookmarkToChangeset, ChangesetInfoMappingToChangesetInfo, ChangesetToChangesetInfoMapping]
  Walking node types [Bookmark, Changeset, ChangesetInfo, ChangesetInfoMapping]
  Final count: (4, 4)
  Bytes/s,* (glob)
  * Type:Walked,Checks,Children Bookmark:1,* Changeset:1,* ChangesetInfo:1,* ChangesetInfoMapping:1,* (glob)

count-objects, deep walk across bonsai and changeset_info
  $ mononoke_walker --readonly-storage scrub -q --bookmark master_bookmark -I deep -i bonsai -i derived_changeset_info 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [BookmarkToChangeset, ChangesetInfoMappingToChangesetInfo, ChangesetInfoToChangesetInfoParent, ChangesetToBonsaiParent, ChangesetToChangesetInfoMapping]
  Walking node types [Bookmark, Changeset, ChangesetInfo, ChangesetInfoMapping]
  Final count: (10, 10)
  Bytes/s,* (glob)
  * Type:Walked,Checks,Children Bookmark:1,* Changeset:3,* ChangesetInfo:3,* ChangesetInfoMapping:3,* (glob)

count-objects, shallow walk across bonsai and unodes
  $ mononoke_walker --readonly-storage scrub -q --bookmark master_bookmark -I shallow -i bonsai -i derived_unodes -i FileContent -X ChangesetToFileContent 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [BookmarkToChangeset, ChangesetToUnodeMapping, UnodeFileToFileContent, UnodeManifestToUnodeFileChild, UnodeManifestToUnodeManifestChild, UnodeMappingToRootUnodeManifest]
  Walking node types [Bookmark, Changeset, FileContent, UnodeFile, UnodeManifest, UnodeMapping]
  Final count: (10, 10)
  Bytes/s,* (glob)
  * Type:Walked,Checks,Children Bookmark:1,1,1 Changeset:1,* FileContent:3,* UnodeFile:3,* UnodeManifest:1,* UnodeMapping:1,* (glob)

count-objects, deep walk across bonsai and unodes
  $ mononoke_walker --readonly-storage scrub -q --bookmark master_bookmark -I deep -i bonsai -i derived_unodes -X ChangesetToBonsaiParent 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [BookmarkToChangeset, ChangesetToUnodeMapping, UnodeFileToLinkedChangeset, UnodeFileToUnodeFileParent, UnodeManifestToLinkedChangeset, UnodeManifestToUnodeFileChild, UnodeManifestToUnodeManifestChild, UnodeManifestToUnodeManifestParent, UnodeMappingToRootUnodeManifest]
  Walking node types [Bookmark, Changeset, UnodeFile, UnodeManifest, UnodeMapping]
  Final count: (13, 13)
  Bytes/s,* (glob)
  * Type:Walked,Checks,Children Bookmark:1,1,1 Changeset:3,* UnodeFile:3,* UnodeManifest:3,* UnodeMapping:3,* (glob)

count-objects, shallow walk across blame
  $ mononoke_walker --readonly-storage scrub -q --bookmark master_bookmark -I shallow -i bonsai -i derived_unodes -i derived_blame -X ChangesetToFileContent -X UnodeFileToFileContent 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [BookmarkToChangeset, ChangesetToUnodeMapping, UnodeFileToBlame, UnodeManifestToUnodeFileChild, UnodeManifestToUnodeManifestChild, UnodeMappingToRootUnodeManifest]
  Walking node types [Blame, Bookmark, Changeset, UnodeFile, UnodeManifest, UnodeMapping]
  Final count: (10, 10)
  Bytes/s,* (glob)
  * Type:Walked,Checks,Children Blame:3,* Bookmark:1,1,1 Changeset:1,* UnodeFile:3,* UnodeManifest:1,* UnodeMapping:1,* (glob)

count-objects, deep walk across blame
  $ mononoke_walker --readonly-storage scrub -q --bookmark master_bookmark -I deep -i bonsai -i derived_unodes -i derived_blame -X ChangesetToBonsaiParent -X UnodeFileToLinkedChangeset -X UnodeManifestToLinkedChangeset 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [BlameToChangeset, BookmarkToChangeset, ChangesetToUnodeMapping, UnodeFileToBlame, UnodeFileToUnodeFileParent, UnodeManifestToUnodeFileChild, UnodeManifestToUnodeManifestChild, UnodeManifestToUnodeManifestParent, UnodeMappingToRootUnodeManifest]
  Walking node types [Blame, Bookmark, Changeset, UnodeFile, UnodeManifest, UnodeMapping]
  Final count: (16, 16)
  Bytes/s,* (glob)
  * Type:Walked,Checks,Children Blame:3,* Bookmark:1,1,1 Changeset:3,* UnodeFile:3,* UnodeManifest:3,* UnodeMapping:3,* (glob)

count-objects, shallow walk across deleted files manifest
  $ mononoke_walker --readonly-storage scrub -q --bookmark master_bookmark -I shallow -i bonsai -i derived_deleted_manifest -X ChangesetToFileContent 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [BookmarkToChangeset, ChangesetToDeletedManifestMapping, DeletedManifestMappingToRootDeletedManifest, DeletedManifestToDeletedManifestChild]
  Walking node types [Bookmark, Changeset, DeletedManifest, DeletedManifestMapping]
  Final count: (4, 4)
  Bytes/s,* (glob)
  * Type:Walked,Checks,Children Bookmark:1,1,1 Changeset:1,* DeletedManifest:1,* DeletedManifestMapping:1,* (glob)

count-objects, deep walk across deleted files manifest
  $ mononoke_walker --readonly-storage scrub -q --bookmark master_bookmark -I deep -i bonsai -i derived_deleted_manifest 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [BookmarkToChangeset, ChangesetToBonsaiParent, ChangesetToDeletedManifestMapping, DeletedManifestMappingToRootDeletedManifest, DeletedManifestToDeletedManifestChild, DeletedManifestToLinkedChangeset]
  Walking node types [Bookmark, Changeset, DeletedManifest, DeletedManifestMapping]
  Final count: (8, 8)
  Bytes/s,* (glob)
  * Type:Walked,Checks,Children Bookmark:1,1,1 Changeset:3,* DeletedManifest:1,* DeletedManifestMapping:3,* (glob)

count-objects, shallow walk across skeleton manifest
  $ mononoke_walker --readonly-storage scrub -q --bookmark master_bookmark -I shallow -i bonsai -i derived_skeleton_manifests -X ChangesetToFileContent 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [BookmarkToChangeset, ChangesetToSkeletonManifestMapping, SkeletonManifestMappingToRootSkeletonManifest, SkeletonManifestToSkeletonManifestChild]
  Walking node types [Bookmark, Changeset, SkeletonManifest, SkeletonManifestMapping]
  Final count: (4, 4)
  Bytes/s,* (glob)
  * Type:Walked,Checks,Children Bookmark:1,1,1 Changeset:1,* SkeletonManifest:1,* SkeletonManifestMapping:1,* (glob)

count-objects, deep walk across skeleton manifest
  $ mononoke_walker --readonly-storage scrub -q --bookmark master_bookmark -I deep -i bonsai -i derived_skeleton_manifests 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [BookmarkToChangeset, ChangesetToBonsaiParent, ChangesetToSkeletonManifestMapping, SkeletonManifestMappingToRootSkeletonManifest, SkeletonManifestToSkeletonManifestChild]
  Walking node types [Bookmark, Changeset, SkeletonManifest, SkeletonManifestMapping]
  Final count: (10, 10)
  Bytes/s,* (glob)
  * Type:Walked,Checks,Children Bookmark:1,1,1 Changeset:3,* SkeletonManifest:3,* SkeletonManifestMapping:3,* (glob)

count-objects, shallow walk across fastlog
  $ mononoke_walker --readonly-storage scrub -q --bookmark master_bookmark -I shallow -i bonsai -i derived_unodes -i derived_fastlog -X ChangesetToFileContent -X UnodeFileToFileContent 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [BookmarkToChangeset, ChangesetToUnodeMapping, FastlogBatchToPreviousBatch, FastlogDirToPreviousBatch, FastlogFileToPreviousBatch, UnodeFileToFastlogFile, UnodeManifestToFastlogDir, UnodeManifestToUnodeFileChild, UnodeManifestToUnodeManifestChild, UnodeMappingToRootUnodeManifest]
  Walking node types [Bookmark, Changeset, FastlogBatch, FastlogDir, FastlogFile, UnodeFile, UnodeManifest, UnodeMapping]
  Final count: (11, 11)
  Bytes/s,* (glob)
  * Type:Walked,Checks,Children Bookmark:1,1,1 Changeset:1,* FastlogDir:1,* FastlogFile:3,* UnodeFile:3,* UnodeManifest:1,* UnodeMapping:1,* (glob)

count-objects, deep walk across fastlog
  $ mononoke_walker --readonly-storage scrub -q --bookmark master_bookmark -I deep -i bonsai -i derived_unodes -i derived_fastlog -X ChangesetToBonsaiParent -X UnodeFileToLinkedChangeset -X UnodeManifestToLinkedChangeset 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types [BookmarkToChangeset, ChangesetToUnodeMapping, FastlogBatchToChangeset, FastlogBatchToPreviousBatch, FastlogDirToChangeset, FastlogDirToPreviousBatch, FastlogFileToChangeset, FastlogFileToPreviousBatch, UnodeFileToFastlogFile, UnodeFileToUnodeFileParent, UnodeManifestToFastlogDir, UnodeManifestToUnodeFileChild, UnodeManifestToUnodeManifestChild, UnodeManifestToUnodeManifestParent, UnodeMappingToRootUnodeManifest]
  Walking node types [Bookmark, Changeset, FastlogBatch, FastlogDir, FastlogFile, UnodeFile, UnodeManifest, UnodeMapping]
  Final count: (19, 19)
  Bytes/s,* (glob)
  * Type:Walked,Checks,Children Bookmark:1,1,1 Changeset:3,* FastlogDir:3,* FastlogFile:3,* UnodeFile:3,* UnodeManifest:3,* UnodeMapping:3,* (glob)
