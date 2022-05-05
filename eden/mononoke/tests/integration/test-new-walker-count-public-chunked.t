# Copyright (c) Meta Platforms, Inc. and affiliates.
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

bonsai core data, deep, unchunked. This is the base case
  $ mononoke_walker -L sizing scrub -q -b master_bookmark -I bonsai 2>&1 | strip_glog
  Walking edge types [BookmarkToChangeset, ChangesetToBonsaiParent, ChangesetToFileContent]
  Walking node types [Bookmark, Changeset, FileContent]
  Seen,Loaded: 7,7
  * Type:Walked,Checks,Children Bookmark:1,* Changeset:3,* FileContent:3,* (glob)

bonsai core data, chunked, shallow.  Shallow walk with chunked commits should still visit all changesets, but no bookmark
  $ mononoke_walker -L sizing -L chunking scrub -q -p Changeset --chunk-size=2 -I shallow -i bonsai -i FileContent 2>&1 | strip_glog
  Walking edge types [ChangesetToFileContent]
  Walking node types [Changeset, FileContent]
  Seen,Loaded: 4,4
  * Type:Walked,Checks,Children Changeset:2,* FileContent:2,* (glob)
  Deferred: 0
  Seen,Loaded: 2,2
  * Type:Walked,Checks,Children Changeset:3,* FileContent:3,* (glob)
  Deferred: 0

oldest, bonsai core data, chunked, shallow. For a shallow walk we should see no difference counts OldestFirst vs NewestFirst
  $ mononoke_walker -l loaded scrub -q -p Changeset --chunk-size=2 -d OldestFirst -I shallow -i bonsai -i FileContent 2>&1 | strip_glog
  Seen,Loaded: 4,4
  Deferred: 0
  Seen,Loaded: 2,2
  Deferred: 0

bonsai core data, chunked, deep. Should still visit all changesets, but no bookmark, second chunk has one deferred edge to process
  $ mononoke_walker -L sizing -L chunking scrub -q -p Changeset --chunk-size=2 -I deep -i bonsai -i FileContent 2>&1 | strip_glog
  Walking edge types [ChangesetToBonsaiParent, ChangesetToFileContent]
  Walking node types [Changeset, FileContent]
  Seen,Loaded: 4,4
  * Type:Walked,Checks,Children Changeset:2,* FileContent:2,* (glob)
  Deferred: 1
  Seen,Loaded: 3,3
  * Type:Walked,Checks,Children Changeset:4,* FileContent:3,* (glob)
  Deferred: 0

oldest, bonsai core data, chunked, deep. Should still visit all changesets. Expect no deferred edges as OldestFirst (parents always point to edges already walked)
  $ mononoke_walker -l loaded scrub -q -p Changeset --chunk-size=2 -d OldestFirst -I deep -i bonsai -i FileContent 2>&1 | strip_glog
  Seen,Loaded: 4,4
  Deferred: 0
  Seen,Loaded: 2,2
  Deferred: 0

hg file content, chunked, deep.  Expect deferred as hg changeset parents will point outside chunk
  $ mononoke_walker -L sizing -L chunking scrub -q -p BonsaiHgMapping --chunk-size=2 -I deep -i hg -i FileContent -x HgFileNode -x HgManifestFileNode 2>&1 | strip_glog
  Walking edge types [BonsaiHgMappingToHgChangesetViaBonsai, HgChangesetToHgManifest, HgChangesetToHgParent, HgChangesetViaBonsaiToHgChangeset, HgFileEnvelopeToFileContent, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope]
  Walking node types [BonsaiHgMapping, FileContent, HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgManifest]
  Seen,Loaded: 15,14
  * Type:Walked,Checks,Children BonsaiHgMapping:2,* FileContent:3,* HgChangeset:2,* HgChangesetViaBonsai:3,* HgFileEnvelope:3,* HgManifest:2,* (glob)
  Deferred: 1
  Seen,Loaded: 4,4
  * Type:Walked,Checks,Children BonsaiHgMapping:3,* FileContent:3,* HgChangeset:3,* HgChangesetViaBonsai:4,* HgFileEnvelope:3,* HgManifest:3,* (glob)
  Deferred: 0

oldest, hg file content, chunked, deep.  Expect no deferred edges as OldestFirst
  $ mononoke_walker -l loaded scrub -q -p BonsaiHgMapping --chunk-size=2 -d OldestFirst -I deep -i hg -i FileContent -x HgFileNode -x HgManifestFileNode 2>&1 | strip_glog
  Seen,Loaded: 12,12
  Deferred: 0
  Seen,Loaded: 6,6
  Deferred: 0

hg file node, chunked, deep.  Expect deferred as hg file node parents will point outside chunk
  $ mononoke_walker -L sizing -L chunking scrub -q -p BonsaiHgMapping --chunk-size=2 -I deep -i hg -x HgFileEnvelope -X HgChangesetToHgParent -X HgFileNodeToLinkedHgBonsaiMapping -X HgFileNodeToLinkedHgChangeset -X HgManifestFileNodeToLinkedHgBonsaiMapping -X HgManifestFileNodeToLinkedHgChangeset 2>&1 | strip_glog
  Walking edge types [BonsaiHgMappingToHgChangesetViaBonsai, HgChangesetToHgManifest, HgChangesetToHgManifestFileNode, HgChangesetViaBonsaiToHgChangeset, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgManifestFileNodeToHgCopyfromFileNode, HgManifestFileNodeToHgParentFileNode, HgManifestToChildHgManifest, HgManifestToHgFileNode]
  Walking node types [BonsaiHgMapping, HgChangeset, HgChangesetViaBonsai, HgFileNode, HgManifest, HgManifestFileNode]
  Seen,Loaded: 14,12
  * Type:Walked,Checks,Children BonsaiHgMapping:2,* HgChangeset:2,* HgChangesetViaBonsai:2,* HgFileNode:3,* HgManifest:2,* HgManifestFileNode:3,* (glob)
  Deferred: 1
  Seen,Loaded: 6,6
  * Type:Walked,Checks,Children BonsaiHgMapping:3,* HgChangeset:3,* HgChangesetViaBonsai:3,* HgFileNode:4,* HgManifest:3,* HgManifestFileNode:4,* (glob)
  Deferred: 0

oldest, hg file node, chunked, deep.  Expect deferred as hg file node parents will point outside chunk
  $ mononoke_walker -loaded scrub -q -p BonsaiHgMapping --chunk-size=2 -d OldestFirst -I deep -i hg -x HgFileEnvelope -X HgChangesetToHgParent -X HgFileNodeToLinkedHgBonsaiMapping -X HgFileNodeToLinkedHgChangeset -X HgManifestFileNodeToLinkedHgBonsaiMapping -X HgManifestFileNodeToLinkedHgChangeset 2>&1 | strip_glog

derived changeset_info, chunked, deep
  $ mononoke_walker -L sizing -L chunking scrub -q -p ChangesetInfoMapping --chunk-size=2 -I deep -i derived_changeset_info 2>&1 | strip_glog
  Walking edge types [ChangesetInfoMappingToChangesetInfo, ChangesetInfoToChangesetInfoParent]
  Walking node types [ChangesetInfo, ChangesetInfoMapping]
  Seen,Loaded: 4,4
  * Type:Walked,Checks,Children ChangesetInfo:2,* ChangesetInfoMapping:2,* (glob)
  Deferred: 1
  Seen,Loaded: 3,3
  * Type:Walked,Checks,Children ChangesetInfo:4,* ChangesetInfoMapping:3,* (glob)
  Deferred: 0

derived deleted_manifest, chunked, deep.  No deferred as there is no parent lookup in the walk
  $ mononoke_walker -L sizing -L chunking scrub -q -p DeletedManifestV2Mapping  --chunk-size=2 -I deep -i derived_deleted_manifest 2>&1 | strip_glog
  Walking edge types [DeletedManifestToDeletedManifestChild, DeletedManifestV2MappingToRootDeletedManifestV2, DeletedManifestV2ToDeletedManifestV2Child]
  Walking node types [DeletedManifest, DeletedManifestV2, DeletedManifestV2Mapping]
  Seen,Loaded: 3,3
  * Type:Walked,Checks,Children* DeletedManifestV2:1,* DeletedManifestV2Mapping:2,* (glob)
  Deferred: 0
  Seen,Loaded: 1,1
  * Type:Walked,Checks,Children* DeletedManifestV2:1,* DeletedManifestV2Mapping:3,* (glob)
  Deferred: 0

derived fsnodes, chunked, deep.  No deferred as there is no parent lookup in the walk
  $ mononoke_walker -L sizing -L chunking scrub -q -p FsnodeMapping --chunk-size=2 -I deep -i derived_fsnodes 2>&1 | strip_glog
  Walking edge types [FsnodeMappingToRootFsnode, FsnodeToChildFsnode]
  Walking node types [Fsnode, FsnodeMapping]
  Seen,Loaded: 4,4
  * Type:Walked,Checks,Children Fsnode:2,* FsnodeMapping:2,* (glob)
  Deferred: 0
  Seen,Loaded: 2,2
  * Type:Walked,Checks,Children Fsnode:3,* FsnodeMapping:3,* (glob)
  Deferred: 0

derived skeleton_manifests, chunked, deep.  No deferred as there is no parent lookup in the walk
  $ mononoke_walker -L sizing -L chunking scrub -q -p SkeletonManifestMapping --chunk-size=2 -I deep -i derived_skeleton_manifests 2>&1 | strip_glog
  Walking edge types [SkeletonManifestMappingToRootSkeletonManifest, SkeletonManifestToSkeletonManifestChild]
  Walking node types [SkeletonManifest, SkeletonManifestMapping]
  Seen,Loaded: 4,4
  * Type:Walked,Checks,Children SkeletonManifest:2,* SkeletonManifestMapping:2,* (glob)
  Deferred: 0
  Seen,Loaded: 2,2
  * Type:Walked,Checks,Children SkeletonManifest:3,* SkeletonManifestMapping:3,* (glob)
  Deferred: 0

derived unodes, chunked, deep. Expect deferred as unode parent will attempt to step outside chunk
  $ mononoke_walker -L sizing scrub -q -p UnodeMapping --chunk-size=2 -I deep -i derived_unodes 2>&1 | strip_glog
  Walking edge types [UnodeFileToUnodeFileParent, UnodeManifestToUnodeFileChild, UnodeManifestToUnodeManifestChild, UnodeManifestToUnodeManifestParent, UnodeMappingToRootUnodeManifest]
  Walking node types [UnodeFile, UnodeManifest, UnodeMapping]
  Repo bounds: (1, 4)
  Starting chunk 1 with bounds (2, 4)
  Seen,Loaded: 8,6
  * Type:Walked,Checks,Children UnodeFile:3,* UnodeManifest:3,* UnodeMapping:2,* (glob)
  Deferred: 1
  Starting chunk 2 with bounds (1, 2)
  Seen,Loaded: 3,3
  * Type:Walked,Checks,Children UnodeFile:4,* UnodeManifest:4,* UnodeMapping:3,* (glob)
  Deferred: 0
  Completed in 2 chunks of size 2

walk with explicit repo bounds, e.g. to reproduce an issue in chunk with bounds 2, 4
  $ mononoke_walker -L sizing scrub -q -p UnodeMapping --repo-lower-bound=2 --repo-upper-bound=4 --chunk-size=2 -I deep -i derived_unodes 2>&1 | strip_glog
  Walking edge types [UnodeFileToUnodeFileParent, UnodeManifestToUnodeFileChild, UnodeManifestToUnodeManifestChild, UnodeManifestToUnodeManifestParent, UnodeMappingToRootUnodeManifest]
  Walking node types [UnodeFile, UnodeManifest, UnodeMapping]
  Repo bounds: (2, 4)
  Starting chunk 1 with bounds (2, 4)
  Seen,Loaded: 8,6
  * Type:Walked,Checks,Children UnodeFile:3,* UnodeManifest:3,* UnodeMapping:2,* (glob)
  Deferred: 1
  Deferred edge counts by type were: UnodeManifestToUnodeFileChild:1 UnodeManifestToUnodeManifestParent:1
  Completed in 1 chunks of size 2

derived unodes, chunked, deep with clearing between chunks. Expect more reloaded in second chunk, but not a full reload
  $ mononoke_walker -L sizing scrub -q -p UnodeMapping --chunk-clear-sample-rate=1 --chunk-size=2 -I deep -i derived_unodes 2>&1 | strip_glog
  Walking edge types [UnodeFileToUnodeFileParent, UnodeManifestToUnodeFileChild, UnodeManifestToUnodeManifestChild, UnodeManifestToUnodeManifestParent, UnodeMappingToRootUnodeManifest]
  Walking node types [UnodeFile, UnodeManifest, UnodeMapping]
  Repo bounds: (1, 4)
  Starting chunk 1 with bounds (2, 4)
  Seen,Loaded: 8,6
  * Type:Walked,Checks,Children UnodeFile:3,* UnodeManifest:3,* UnodeMapping:2,* (glob)
  Deferred: 1
  Clearing state after chunk 1
  Starting chunk 2 with bounds (1, 2)
  Seen,Loaded: 5,5
  * Type:Walked,Checks,Children UnodeFile:5,* UnodeManifest:5,* UnodeMapping:3,* (glob)
  Deferred: 0
  Clearing state after chunk 2
  Completed in 2 chunks of size 2

derived unodes blame, chunked, deep. Expect deferred as blame entry will attempt to step outside chunk
  $ mononoke_walker -L sizing -L chunking scrub -q -p UnodeMapping --chunk-size=2 -I deep -i derived_unodes -i derived_blame -X UnodeFileToUnodeFileParent -X UnodeManifestToUnodeManifestParent 2>&1 | strip_glog
  Walking edge types [UnodeFileToBlame, UnodeManifestToUnodeFileChild, UnodeManifestToUnodeManifestChild, UnodeMappingToRootUnodeManifest]
  Walking node types [Blame, UnodeFile, UnodeManifest, UnodeMapping]
  Seen,Loaded: 9,8
  * Type:Walked,Checks,Children Blame:2,* UnodeFile:3,* UnodeManifest:2,* UnodeMapping:2,* (glob)
  Deferred: 1
  Seen,Loaded: 4,4
  * Type:Walked,Checks,Children Blame:3,* UnodeFile:4,* UnodeManifest:3,* UnodeMapping:3,* (glob)
  Deferred: 0

derived unodes fastlog, chunked, deep. Expect deferred as fastlog entry will attempt to step outside chunk
  $ mononoke_walker -L sizing -L chunking scrub -q -p UnodeMapping --chunk-size=2 -I deep -i derived_unodes -i derived_fastlog -X UnodeFileToUnodeFileParent -X UnodeManifestToUnodeManifestParent 2>&1 | strip_glog
  Walking edge types [FastlogBatchToPreviousBatch, FastlogDirToPreviousBatch, FastlogFileToPreviousBatch, UnodeFileToFastlogFile, UnodeManifestToFastlogDir, UnodeManifestToUnodeFileChild, UnodeManifestToUnodeManifestChild, UnodeMappingToRootUnodeManifest]
  Walking node types [FastlogBatch, FastlogDir, FastlogFile, UnodeFile, UnodeManifest, UnodeMapping]
  Seen,Loaded: 11,10
  * Type:Walked,Checks,Children FastlogBatch:0,* FastlogDir:2,* FastlogFile:2,* UnodeFile:3,* UnodeManifest:2,* UnodeMapping:2,* (glob)
  Deferred: 1
  Seen,Loaded: 5,5
  * Type:Walked,Checks,Children FastlogBatch:0,* FastlogDir:3,* FastlogFile:3,* UnodeFile:4,* UnodeManifest:3,* UnodeMapping:3,* (glob)
  Deferred: 0
