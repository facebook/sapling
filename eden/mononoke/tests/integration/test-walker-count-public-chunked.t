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

bonsai core data, deep, unchunked. This is the base case
  $ mononoke_walker -L sizing scrub -q -b master_bookmark -I bonsai 2>&1 | strip_glog
  Walking edge types [BookmarkToChangeset, ChangesetToBonsaiParent, ChangesetToFileContent]
  Walking node types [Bookmark, Changeset, FileContent]
  Seen,Loaded: 7,7
  * Type:Walked,Checks,Children Bookmark:1,1,2 Changeset:3,* FileContent:3,3,0 (glob)

bonsai core data, chunked, shallow.  Shallow walk with chunked commits should still visit all changesets, but no bookmark
  $ mononoke_walker -L sizing scrub -q -p Changeset --chunk-size=2 -I shallow -i bonsai -i FileContent 2>&1 | strip_glog
  Walking edge types [ChangesetToFileContent]
  Walking node types [Changeset, FileContent]
  Seen,Loaded: 4,4
  * Type:Walked,Checks,Children Changeset:2,*,4 FileContent:2,2,0 (glob)
  Deferred: 0
  Seen,Loaded: 2,2
  * Type:Walked,Checks,Children Changeset:3,*,6 FileContent:3,3,0 (glob)
  Deferred: 0
  Completed in 2 chunks of 2

bonsai core data, chunked, deep. Should still visit all changesets, but no bookmark, second chunk has one deferred edge to process
  $ mononoke_walker -L sizing scrub -q -p Changeset --chunk-size=2 -I deep -i bonsai -i FileContent 2>&1 | strip_glog
  Walking edge types [ChangesetToBonsaiParent, ChangesetToFileContent]
  Walking node types [Changeset, FileContent]
  Seen,Loaded: 4,4
  * Type:Walked,Checks,Children Changeset:2,*,4 FileContent:2,2,0 (glob)
  Deferred: 1
  Seen,Loaded: 3,3
  * Type:Walked,Checks,Children Changeset:4,*,6 FileContent:3,*,0 (glob)
  Deferred: 0
  Completed in 2 chunks of 2

derived changeset_info, chunked, deep
  $ mononoke_walker -L sizing scrub -q -p ChangesetInfoMapping --chunk-size=2 -I deep -i derived_changeset_info 2>&1 | strip_glog
  Walking edge types [ChangesetInfoMappingToChangesetInfo, ChangesetInfoToChangesetInfoParent]
  Walking node types [ChangesetInfo, ChangesetInfoMapping]
  Seen,Loaded: 4,4
  * Type:Walked,Checks,Children ChangesetInfo:2,*,0 ChangesetInfoMapping:2,*,4 (glob)
  Deferred: 1
  Seen,Loaded: 3,3
  * Type:Walked,Checks,Children ChangesetInfo:4,*,0 ChangesetInfoMapping:3,*,6 (glob)
  Deferred: 0
  Completed in 2 chunks of 2

derived deleted_manifest, chunked, deep.  No deferred as there is no parent lookup in the walk
  $ mononoke_walker -L sizing scrub -q -p DeletedManifestMapping --chunk-size=2 -I deep -i derived_deleted_manifest 2>&1 | strip_glog
  Walking edge types [DeletedManifestMappingToRootDeletedManifest, DeletedManifestToDeletedManifestChild]
  Walking node types [DeletedManifest, DeletedManifestMapping]
  Seen,Loaded: 3,3
  * Type:Walked,Checks,Children DeletedManifest:1,*,0 DeletedManifestMapping:2,*,3 (glob)
  Deferred: 0
  Seen,Loaded: 1,1
  * Type:Walked,Checks,Children DeletedManifest:1,*,0 DeletedManifestMapping:3,*,4 (glob)
  Deferred: 0
  Completed in 2 chunks of 2

derived fsnodes, chunked, deep.  No deferred as there is no parent lookup in the walk
  $ mononoke_walker -L sizing scrub -q -p FsnodeMapping --chunk-size=2 -I deep -i derived_fsnodes 2>&1 | strip_glog
  Walking edge types [FsnodeMappingToRootFsnode, FsnodeToChildFsnode]
  Walking node types [Fsnode, FsnodeMapping]
  Seen,Loaded: 4,4
  * Type:Walked,Checks,Children Fsnode:2,2,0 FsnodeMapping:2,2,4 (glob)
  Deferred: 0
  Seen,Loaded: 2,2
  * Type:Walked,Checks,Children Fsnode:3,3,0 FsnodeMapping:3,3,6 (glob)
  Deferred: 0
  Completed in 2 chunks of 2

derived skeleton_manifests, chunked, deep.  No deferred as there is no parent lookup in the walk
  $ mononoke_walker -L sizing scrub -q -p SkeletonManifestMapping --chunk-size=2 -I deep -i derived_skeleton_manifests 2>&1 | strip_glog
  Walking edge types [SkeletonManifestMappingToRootSkeletonManifest, SkeletonManifestToSkeletonManifestChild]
  Walking node types [SkeletonManifest, SkeletonManifestMapping]
  Seen,Loaded: 4,4
  * Type:Walked,Checks,Children SkeletonManifest:2,*,0 SkeletonManifestMapping:2,*,4 (glob)
  Deferred: 0
  Seen,Loaded: 2,2
  * Type:Walked,Checks,Children SkeletonManifest:3,*,0 SkeletonManifestMapping:3,*,6 (glob)
  Deferred: 0
  Completed in 2 chunks of 2

derived unodes, chunked, deep. Expect deferred as unode parent will attempt to step outside chunk
  $ mononoke_walker -L sizing scrub -q -p UnodeMapping --chunk-size=2 -I deep -i derived_unodes 2>&1 | strip_glog
  Walking edge types [UnodeFileToUnodeFileParent, UnodeManifestToUnodeFileChild, UnodeManifestToUnodeManifestChild, UnodeManifestToUnodeManifestParent, UnodeMappingToRootUnodeManifest]
  Walking node types [UnodeFile, UnodeManifest, UnodeMapping]
  Seen,Loaded: 8,6
  * Type:Walked,Checks,Children UnodeFile:3,*,0 UnodeManifest:3,*,4 UnodeMapping:2,*,4 (glob)
  Deferred: 1
  Seen,Loaded: 3,3
  * Type:Walked,Checks,Children UnodeFile:4,*,0 UnodeManifest:4,*,4 UnodeMapping:3,*,5 (glob)
  Deferred: 0
  Completed in 2 chunks of 2

derived unodes blame, chunked, deep. Expect deferred as blame entry will attempt to step outside chunk
  $ mononoke_walker -L sizing scrub -q -p UnodeMapping --chunk-size=2 -I deep -i derived_unodes -i derived_blame -X UnodeFileToUnodeFileParent -X UnodeManifestToUnodeManifestParent 2>&1 | strip_glog
  Walking edge types [UnodeFileToBlame, UnodeManifestToUnodeFileChild, UnodeManifestToUnodeManifestChild, UnodeMappingToRootUnodeManifest]
  Walking node types [Blame, UnodeFile, UnodeManifest, UnodeMapping]
  Seen,Loaded: 9,8
  * Type:Walked,Checks,Children Blame:2,* UnodeFile:3,* UnodeManifest:2,* UnodeMapping:2,*,4 (glob)
  Deferred: 1
  Seen,Loaded: 4,4
  * Type:Walked,Checks,Children Blame:3,* UnodeFile:4,* UnodeManifest:3,* UnodeMapping:3,*,6 (glob)
  Deferred: 0
  Completed in 2 chunks of 2

derived unodes fastlog, chunked, deep. Expect deferred as fastlog entry will attempt to step outside chunk
  $ mononoke_walker -L sizing scrub -q -p UnodeMapping --chunk-size=2 -I deep -i derived_unodes -i derived_fastlog -X UnodeFileToUnodeFileParent -X UnodeManifestToUnodeManifestParent 2>&1 | strip_glog
  Walking edge types [FastlogBatchToPreviousBatch, FastlogDirToPreviousBatch, FastlogFileToPreviousBatch, UnodeFileToFastlogFile, UnodeManifestToFastlogDir, UnodeManifestToUnodeFileChild, UnodeManifestToUnodeManifestChild, UnodeMappingToRootUnodeManifest]
  Walking node types [FastlogBatch, FastlogDir, FastlogFile, UnodeFile, UnodeManifest, UnodeMapping]
  Seen,Loaded: 11,10
  * Type:Walked,Checks,Children FastlogBatch:0,0,0 FastlogDir:2,* FastlogFile:2,* UnodeFile:3,* UnodeManifest:2,* UnodeMapping:2,2,4 (glob)
  Deferred: 1
  Seen,Loaded: 5,5
  * Type:Walked,Checks,Children FastlogBatch:0,0,0 FastlogDir:3,* FastlogFile:3,* UnodeFile:4,* UnodeManifest:3,* UnodeMapping:3,3,6 (glob)
  Deferred: 0
  Completed in 2 chunks of 2
