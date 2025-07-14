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
  $ blobimport repo/.hg repo --derived-data-type=fsnodes

check blobstore numbers, walk will do some more steps for mappings
  $ BLOBPREFIX="$TESTTMP/blobstore/blobs/blob-repo0000"
  $ WALKABLEBLOBCOUNT=$(ls $BLOBPREFIX.* | grep -v .filenode_lookup. | wc -l)
  $ echo "$WALKABLEBLOBCOUNT"
  36
  $ find $TESTTMP/blobstore/blobs/ -type f ! -path "*.filenode_lookup.*" -exec du --bytes -c {} + | tail -1 | cut -f1
  3051

Base case, sample all in one go. Expeding WALKABLEBLOBCOUNT keys plus mappings and root.  Note that the total is 3086, but blobs are 2805. This is due to BonsaiHgMapping loading the hg changeset
  $ mononoke_walker -l sizing corpus -q -b master_bookmark --output-dir=full --sample-rate 1 -I deep -i default -i derived_fsnodes 2>&1 | grep -vE "(Bytes|Walked)/s"
  [INFO] Walking edge types [AliasContentMappingToFileContent, BonsaiHgMappingToHgChangesetViaBonsai, BookmarkToChangeset, ChangesetToBonsaiHgMapping, ChangesetToBonsaiParent, ChangesetToFileContent, ChangesetToFsnodeMapping, FileContentMetadataV2ToGitSha1Alias, FileContentMetadataV2ToSeededBlake3Alias, FileContentMetadataV2ToSha1Alias, FileContentMetadataV2ToSha256Alias, FileContentToFileContentMetadataV2, FsnodeMappingToRootFsnode, FsnodeToChildFsnode, FsnodeToFileContent, HgBonsaiMappingToChangeset, HgChangesetToHgManifest, HgChangesetToHgParent, HgChangesetViaBonsaiToHgChangeset, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgBonsaiMapping, HgFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  [INFO] Walking node types [AliasContentMapping, BonsaiHgMapping, Bookmark, Changeset, FileContent, FileContentMetadataV2, Fsnode, FsnodeMapping, HgBonsaiMapping, HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgFileNode, HgManifest]
  [INFO] [walker corpus{repo=repo}] Seen,Loaded: 49,49

Check the corpus dumped to disk agrees with the walk stats
  $ for x in full/*; do size=$(find $x -type f -exec du --bytes -c {} + | tail -1 | cut -f1); if [[ -n "$size" ]]; then echo "$x $size"; fi; done
  full/AliasContentMapping 444
  full/BonsaiHgMapping 281
  full/Changeset 277
  full/FileContent 12
  full/FileContentMetadataV2 486
  full/Fsnode 822
  full/FsnodeMapping 96
  full/HgChangeset 281
  full/HgFileEnvelope 189
  full/HgManifest 444

Repeat but using the sample-offset to slice.  Offset zero will tend to be larger as root paths sample as zero. 2000+475+611=3086
  $ for i in {0..2}; do mkdir -p slice/$i; echo slice $i; mononoke_walker -L graph corpus -q -b master_bookmark -I deep -i default -i derived_fsnodes --output-dir=slice/$i --sample-rate=3 --sample-offset=$i 2>&1; done | grep -vE "(Bytes|Walked)/s"
  slice 0
  [INFO] Walking edge types [AliasContentMappingToFileContent, BonsaiHgMappingToHgChangesetViaBonsai, BookmarkToChangeset, ChangesetToBonsaiHgMapping, ChangesetToBonsaiParent, ChangesetToFileContent, ChangesetToFsnodeMapping, FileContentMetadataV2ToGitSha1Alias, FileContentMetadataV2ToSeededBlake3Alias, FileContentMetadataV2ToSha1Alias, FileContentMetadataV2ToSha256Alias, FileContentToFileContentMetadataV2, FsnodeMappingToRootFsnode, FsnodeToChildFsnode, FsnodeToFileContent, HgBonsaiMappingToChangeset, HgChangesetToHgManifest, HgChangesetToHgParent, HgChangesetViaBonsaiToHgChangeset, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgBonsaiMapping, HgFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  [INFO] Walking node types [AliasContentMapping, BonsaiHgMapping, Bookmark, Changeset, FileContent, FileContentMetadataV2, Fsnode, FsnodeMapping, HgBonsaiMapping, HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgFileNode, HgManifest]
  [INFO] [walker corpus{repo=repo}] Seen,Loaded: 49,49
  slice 1
  [INFO] Walking edge types [AliasContentMappingToFileContent, BonsaiHgMappingToHgChangesetViaBonsai, BookmarkToChangeset, ChangesetToBonsaiHgMapping, ChangesetToBonsaiParent, ChangesetToFileContent, ChangesetToFsnodeMapping, FileContentMetadataV2ToGitSha1Alias, FileContentMetadataV2ToSeededBlake3Alias, FileContentMetadataV2ToSha1Alias, FileContentMetadataV2ToSha256Alias, FileContentToFileContentMetadataV2, FsnodeMappingToRootFsnode, FsnodeToChildFsnode, FsnodeToFileContent, HgBonsaiMappingToChangeset, HgChangesetToHgManifest, HgChangesetToHgParent, HgChangesetViaBonsaiToHgChangeset, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgBonsaiMapping, HgFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  [INFO] Walking node types [AliasContentMapping, BonsaiHgMapping, Bookmark, Changeset, FileContent, FileContentMetadataV2, Fsnode, FsnodeMapping, HgBonsaiMapping, HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgFileNode, HgManifest]
  [INFO] [walker corpus{repo=repo}] Seen,Loaded: 49,49
  slice 2
  [INFO] Walking edge types [AliasContentMappingToFileContent, BonsaiHgMappingToHgChangesetViaBonsai, BookmarkToChangeset, ChangesetToBonsaiHgMapping, ChangesetToBonsaiParent, ChangesetToFileContent, ChangesetToFsnodeMapping, FileContentMetadataV2ToGitSha1Alias, FileContentMetadataV2ToSeededBlake3Alias, FileContentMetadataV2ToSha1Alias, FileContentMetadataV2ToSha256Alias, FileContentToFileContentMetadataV2, FsnodeMappingToRootFsnode, FsnodeToChildFsnode, FsnodeToFileContent, HgBonsaiMappingToChangeset, HgChangesetToHgManifest, HgChangesetToHgParent, HgChangesetViaBonsaiToHgChangeset, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgBonsaiMapping, HgFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  [INFO] Walking node types [AliasContentMapping, BonsaiHgMapping, Bookmark, Changeset, FileContent, FileContentMetadataV2, Fsnode, FsnodeMapping, HgBonsaiMapping, HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgFileNode, HgManifest]
  [INFO] [walker corpus{repo=repo}] Seen,Loaded: 49,49

See the breakdown
  $ for x in slice/*/*; do size=$(find $x -type f -exec du --bytes -c {} + | tail -1 | cut -f1); if [[ -n "$size" ]]; then echo "$x $size"; fi; done
  slice/0/AliasContentMapping 148
  slice/0/BonsaiHgMapping 101
  slice/0/Changeset 104
  slice/0/FileContent 4
  slice/0/FileContentMetadataV2 162
  slice/0/Fsnode 822
  slice/0/FsnodeMapping 32
  slice/0/HgChangeset 202
  slice/0/HgFileEnvelope 63
  slice/0/HgManifest 444
  slice/1/AliasContentMapping 148
  slice/1/BonsaiHgMapping 79
  slice/1/Changeset 69
  slice/1/FileContent 4
  slice/1/FileContentMetadataV2 162
  slice/1/FsnodeMapping 32
  slice/1/HgFileEnvelope 63
  slice/2/AliasContentMapping 148
  slice/2/BonsaiHgMapping 101
  slice/2/Changeset 104
  slice/2/FileContent 4
  slice/2/FileContentMetadataV2 162
  slice/2/FsnodeMapping 32
  slice/2/HgChangeset 79
  slice/2/HgFileEnvelope 63

Check overall total
  $ find slice -type f -exec du --bytes -c {} + | tail -1 | cut -f1
  3332

Check path regex can pick out just one path
  $ mononoke_walker -l sizing corpus -q -b master_bookmark --output-dir=A --sample-path-regex='^A$' --sample-rate 1 -I deep -i default -i derived_fsnodes 2>&1 | grep -vE "(Bytes|Walked)/s"
  [INFO] Walking edge types [AliasContentMappingToFileContent, BonsaiHgMappingToHgChangesetViaBonsai, BookmarkToChangeset, ChangesetToBonsaiHgMapping, ChangesetToBonsaiParent, ChangesetToFileContent, ChangesetToFsnodeMapping, FileContentMetadataV2ToGitSha1Alias, FileContentMetadataV2ToSeededBlake3Alias, FileContentMetadataV2ToSha1Alias, FileContentMetadataV2ToSha256Alias, FileContentToFileContentMetadataV2, FsnodeMappingToRootFsnode, FsnodeToChildFsnode, FsnodeToFileContent, HgBonsaiMappingToChangeset, HgChangesetToHgManifest, HgChangesetToHgParent, HgChangesetViaBonsaiToHgChangeset, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgBonsaiMapping, HgFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  [INFO] Walking node types [AliasContentMapping, BonsaiHgMapping, Bookmark, Changeset, FileContent, FileContentMetadataV2, Fsnode, FsnodeMapping, HgBonsaiMapping, HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgFileNode, HgManifest]
  [INFO] [walker corpus{repo=repo}] Seen,Loaded: 49,49
