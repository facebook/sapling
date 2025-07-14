# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Unpack repo with corrupted hg commit - the author has a newline.
repo structure is a follows
> A-B-C
>    \-D
>     \-E-F
> # bookmark: C good
> # bookmark: D bad
> EOF

Commits D and E have corrupted author field containing newline ("te\nst")

NOTE: we're using only the monsql and blobstore dirs from the tarred repo dir.
Configs are generated from scratch so we don't have to update them in fixtures too often

  $ REPONAME="repo" setup_common_config "blob_files"
  $ cd "$TESTTMP"
  $ tar --strip-components=1 -xf "$TEST_FIXTURES/fixtures/repo_with_newline_author_commit.tar.xz" repo_with_newline_author_commit/blobstore repo_with_newline_author_commit/monsql
Check that indeed the bad commit is bad
  $ mononoke_admin fetch -R repo -B bad
  Error: Failed to load changeset c567ecc582f8822cf1529a127dec105db78a440fbeaa21221ce2abc4affff6ec
  
  Caused by:
      0: invalid Thrift structure 'BonsaiChangeset': Invalid changeset
      1: invalid bonsai changeset: commit author contains a newline at offset 2
  [1]



Try scrubbing hg file contents (as we regularly do in production)
  $ mononoke_walker scrub --chunk-direction=OldestFirst --chunk-by-public=BonsaiHgMapping -I deep \
  > -i=bonsai  -i=hg -i=FileContent -x=HgFileNode -I=marker \
  > --exclude-node BonsaiHgMapping:c567ecc582f8822cf1529a127dec105db78a440fbeaa21221ce2abc4affff6ec \
  > --exclude-node BonsaiHgMapping:17090c5bf061aa21f7aa2393796b7bb9a20c81a3940b2aaab7683f1e10c67978 \
  > -q 2>&1 | grep -vE "(Bytes|Walked)/s"
  [INFO] Walking edge types [BonsaiHgMappingToHgChangesetViaBonsai, ChangesetToBonsaiHgMapping, ChangesetToBonsaiParent, ChangesetToFileContent, HgBonsaiMappingToChangeset, HgChangesetToHgManifest, HgChangesetToHgManifestFileNode, HgChangesetToHgParent, HgChangesetViaBonsaiToHgChangeset, HgFileEnvelopeToFileContent, HgManifestFileNodeToHgCopyfromFileNode, HgManifestFileNodeToHgParentFileNode, HgManifestFileNodeToLinkedHgBonsaiMapping, HgManifestFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope]
  [INFO] Walking node types [BonsaiHgMapping, Changeset, FileContent, HgBonsaiMapping, HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgManifest, HgManifestFileNode]
  [INFO] [walker scrub{repo=repo}] Repo bounds: (1, 12)
  [INFO] [walker scrub{repo=repo}] Starting chunk 1 with bounds (1, 12)
  [INFO] [walker scrub{repo=repo}] Suppressing edge OutgoingEdge { label: RootToBonsaiHgMapping, target: BonsaiHgMapping(ChangesetKey { inner: ChangesetId(Blake2(*)), filenode_known_derived: false }), path: None } (glob)
  [INFO] [walker scrub{repo=repo}] Suppressing edge OutgoingEdge { label: RootToBonsaiHgMapping, target: BonsaiHgMapping(ChangesetKey { inner: ChangesetId(Blake2(*)), filenode_known_derived: false }), path: None } (glob)
  [INFO] [walker scrub{repo=repo}] Seen,Loaded: 4,4
  [INFO] [walker scrub{repo=repo}] Deferred: 0
  [INFO] [walker scrub{repo=repo}] Completed in 1 chunks of size 100000

Try scrubbing hg filenodes (as we regularly do in production)
  $ mononoke_walker scrub --chunk-direction=OldestFirst --chunk-by-public=BonsaiHgMapping -I deep \
  > -i=bonsai  -i=hg -i=FileContent -x=HgFileEnvelope -i=HgFileNode \
  > --exclude-node BonsaiHgMapping:c567ecc582f8822cf1529a127dec105db78a440fbeaa21221ce2abc4affff6ec \
  > --exclude-node BonsaiHgMapping:17090c5bf061aa21f7aa2393796b7bb9a20c81a3940b2aaab7683f1e10c67978 \
  > -q 2>&1 | grep -vE "(Bytes|Walked)/s"
  [INFO] Walking edge types [BonsaiHgMappingToHgChangesetViaBonsai, ChangesetToBonsaiHgMapping, ChangesetToBonsaiParent, ChangesetToFileContent, HgBonsaiMappingToChangeset, HgChangesetToHgManifest, HgChangesetToHgManifestFileNode, HgChangesetToHgParent, HgChangesetViaBonsaiToHgChangeset, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgBonsaiMapping, HgFileNodeToLinkedHgChangeset, HgManifestFileNodeToHgCopyfromFileNode, HgManifestFileNodeToHgParentFileNode, HgManifestFileNodeToLinkedHgBonsaiMapping, HgManifestFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileNode]
  [INFO] Walking node types [BonsaiHgMapping, Changeset, FileContent, HgBonsaiMapping, HgChangeset, HgChangesetViaBonsai, HgFileNode, HgManifest, HgManifestFileNode]
  [INFO] [walker scrub{repo=repo}] Repo bounds: (1, 12)
  [INFO] [walker scrub{repo=repo}] Starting chunk 1 with bounds (1, 12)
  [INFO] [walker scrub{repo=repo}] Suppressing edge OutgoingEdge { label: RootToBonsaiHgMapping, target: BonsaiHgMapping(ChangesetKey { inner: ChangesetId(Blake2(*)), filenode_known_derived: false }), path: None } (glob)
  [INFO] [walker scrub{repo=repo}] Suppressing edge OutgoingEdge { label: RootToBonsaiHgMapping, target: BonsaiHgMapping(ChangesetKey { inner: ChangesetId(Blake2(*)), filenode_known_derived: false }), path: None } (glob)
  [INFO] [walker scrub{repo=repo}] Seen,Loaded: 4,4
  [INFO] [walker scrub{repo=repo}] Deferred: 0
  [INFO] [walker scrub{repo=repo}] Completed in 1 chunks of size 100000

Basic case, deep scrub of the good branch still works
  $ mononoke_walker scrub -I deep -q -b good 2>&1 | grep -vE "(Bytes|Walked)/s"
  [INFO] Walking edge types [AliasContentMappingToFileContent, BonsaiHgMappingToHgChangesetViaBonsai, BookmarkToChangeset, ChangesetToBonsaiHgMapping, ChangesetToBonsaiParent, ChangesetToFileContent, FileContentMetadataV2ToGitSha1Alias, FileContentMetadataV2ToSeededBlake3Alias, FileContentMetadataV2ToSha1Alias, FileContentMetadataV2ToSha256Alias, FileContentToFileContentMetadataV2, HgBonsaiMappingToChangeset, HgChangesetToHgManifest, HgChangesetToHgParent, HgChangesetViaBonsaiToHgChangeset, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgBonsaiMapping, HgFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  [INFO] Walking node types [AliasContentMapping, BonsaiHgMapping, Bookmark, Changeset, FileContent, FileContentMetadataV2, HgBonsaiMapping, HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgFileNode, HgManifest]
  [INFO] [walker scrub{repo=repo}] Seen,Loaded: 25,25

Basic case, deep scrub of the bad branch does work only because of current mitigations
and only because the bad commit is a head of the branch.
  $ mononoke_walker scrub -I deep -q -b bad \
  > --exclude-node Changeset:c567ecc582f8822cf1529a127dec105db78a440fbeaa21221ce2abc4affff6ec \
  > 2>&1 | grep -vE "(Bytes|Walked)/s"
  [INFO] Walking edge types [AliasContentMappingToFileContent, BonsaiHgMappingToHgChangesetViaBonsai, BookmarkToChangeset, ChangesetToBonsaiHgMapping, ChangesetToBonsaiParent, ChangesetToFileContent, FileContentMetadataV2ToGitSha1Alias, FileContentMetadataV2ToSeededBlake3Alias, FileContentMetadataV2ToSha1Alias, FileContentMetadataV2ToSha256Alias, FileContentToFileContentMetadataV2, HgBonsaiMappingToChangeset, HgChangesetToHgManifest, HgChangesetToHgParent, HgChangesetViaBonsaiToHgChangeset, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgBonsaiMapping, HgFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  [INFO] Walking node types [AliasContentMapping, BonsaiHgMapping, Bookmark, Changeset, FileContent, FileContentMetadataV2, HgBonsaiMapping, HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgFileNode, HgManifest]
  [INFO] [walker scrub{repo=repo}] Seen,Loaded: 1,1

Basic case, deep scrub of the bad branch that doesn't have bad commit at head.
  $ mononoke_walker scrub -I deep -q -r HgChangeset:d4775aa0d65c35f8b71fbf9ea44a759b8d817ce7 \
  > --exclude-node BonsaiHgMapping:17090c5bf061aa21f7aa2393796b7bb9a20c81a3940b2aaab7683f1e10c67978 \
  > --exclude-node Changeset:17090c5bf061aa21f7aa2393796b7bb9a20c81a3940b2aaab7683f1e10c67978 \
  > --exclude-node HgChangeset:6d7e2d6f0ed4ce975af19d70754704b279e4fd35 \
  > --exclude-node HgChangesetViaBonsai:6d7e2d6f0ed4ce975af19d70754704b279e4fd35 \
  > 2>&1 | grep -vE "(Bytes|Walked)/s"
  [INFO] Walking edge types [AliasContentMappingToFileContent, BonsaiHgMappingToHgChangesetViaBonsai, ChangesetToBonsaiHgMapping, ChangesetToBonsaiParent, ChangesetToFileContent, FileContentMetadataV2ToGitSha1Alias, FileContentMetadataV2ToSeededBlake3Alias, FileContentMetadataV2ToSha1Alias, FileContentMetadataV2ToSha256Alias, FileContentToFileContentMetadataV2, HgBonsaiMappingToChangeset, HgChangesetToHgManifest, HgChangesetToHgParent, HgChangesetViaBonsaiToHgChangeset, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgBonsaiMapping, HgFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  [INFO] Walking node types [AliasContentMapping, BonsaiHgMapping, Changeset, FileContent, FileContentMetadataV2, HgBonsaiMapping, HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgFileNode, HgManifest]
  [INFO] [walker scrub{repo=repo}] Seen,Loaded: 34,34
