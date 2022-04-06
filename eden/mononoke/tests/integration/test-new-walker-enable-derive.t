# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ setup_common_config "$@"

  $ cat >> "$HGRCPATH" <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > [extensions]
  > amend=
  > EOF

  $ hg init repo-hg
  $ cd repo-hg || exit 1
  $ setup_hg_server
  $ drawdag <<EOF
  > C
  > |
  > B
  > |
  > A
  > EOF

  $ hg bookmark master_bookmark -r tip
  $ cd ..
  $ blobimport repo-hg/.hg repo --derived-data-type fsnodes

  $ default_setup_blobimport "blob_files"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting

Check counts
  $ BLOBPREFIX="$TESTTMP/blobstore/blobs/blob-repo0000"
  $ BLOBCOUNT=$(ls $BLOBPREFIX.* | wc -l)
  $ echo "$BLOBCOUNT"
  30

Do a walk of everything, it should all be there
  $ mononoke_new_walker -L sizing scrub -q -b master_bookmark -I deep -I marker -i default -i derived_fsnodes 2>&1 | strip_glog
  Walking edge types [AliasContentMappingToFileContent, BonsaiHgMappingToHgChangesetViaBonsai, BookmarkToChangeset, ChangesetToBonsaiHgMapping, ChangesetToBonsaiParent, ChangesetToFileContent, ChangesetToFsnodeMapping, ChangesetToPhaseMapping, FileContentMetadataToGitSha1Alias, FileContentMetadataToSha1Alias, FileContentMetadataToSha256Alias, FileContentToFileContentMetadata, FsnodeMappingToRootFsnode, FsnodeToChildFsnode, FsnodeToFileContent, HgBonsaiMappingToChangeset, HgChangesetToHgManifest, HgChangesetToHgParent, HgChangesetViaBonsaiToHgChangeset, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgBonsaiMapping, HgFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  Walking node types [AliasContentMapping, BonsaiHgMapping, Bookmark, Changeset, FileContent, FileContentMetadata, Fsnode, FsnodeMapping, HgBonsaiMapping, HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgFileNode, HgManifest, PhaseMapping]
  Seen,Loaded: 46,46
  Walked/s,* (glob)

check the metadata base case
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select count(*) FROM filenodes where repo_id >= 0";
  6
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select count(*)FROM bonsai_hg_mapping where repo_id >= 0";
  3

delete the hg blob forms
  $ ls $BLOBPREFIX.* | grep -E '.(filenode_lookup|hgchangeset|hgfilenode|hgmanifest).' | xargs rm
  $ BLOBCOUNT=$(ls $BLOBPREFIX.* | wc -l)
  $ echo "$BLOBCOUNT"
  18

delete the hg db forms
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM filenodes where repo_id >= 0";
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM bonsai_hg_mapping where repo_id >= 0";

Do a walk again, should succeed but not find the hg types
  $ mononoke_new_walker -L sizing scrub -q -b master_bookmark -I deep -I marker -i default -i derived_fsnodes 2>&1 | strip_glog
  Walking edge types [AliasContentMappingToFileContent, BonsaiHgMappingToHgChangesetViaBonsai, BookmarkToChangeset, ChangesetToBonsaiHgMapping, ChangesetToBonsaiParent, ChangesetToFileContent, ChangesetToFsnodeMapping, ChangesetToPhaseMapping, FileContentMetadataToGitSha1Alias, FileContentMetadataToSha1Alias, FileContentMetadataToSha256Alias, FileContentToFileContentMetadata, FsnodeMappingToRootFsnode, FsnodeToChildFsnode, FsnodeToFileContent, HgBonsaiMappingToChangeset, HgChangesetToHgManifest, HgChangesetToHgParent, HgChangesetViaBonsaiToHgChangeset, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgBonsaiMapping, HgFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  Walking node types [AliasContentMapping, BonsaiHgMapping, Bookmark, Changeset, FileContent, FileContentMetadata, Fsnode, FsnodeMapping, HgBonsaiMapping, HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgFileNode, HgManifest, PhaseMapping]
  Seen,Loaded: 28,28
  * HgBonsaiMapping:0,0,0 HgChangeset:0,0,0 HgChangesetViaBonsai:0,0,0 HgFileEnvelope:0,0,0 HgFileNode:0,0,0 HgManifest:0,0,0 PhaseMapping:3,* (glob)

Do a walk again, with --enable-derive, should succeed with the full count
  $ mononoke_new_walker --with-readonly-storage=false -L sizing scrub -q --enable-derive -b master_bookmark -I deep -I marker -i default -i derived_fsnodes 2>&1 | strip_glog
  Walking edge types [AliasContentMappingToFileContent, BonsaiHgMappingToHgChangesetViaBonsai, BookmarkToChangeset, ChangesetToBonsaiHgMapping, ChangesetToBonsaiParent, ChangesetToFileContent, ChangesetToFsnodeMapping, ChangesetToPhaseMapping, FileContentMetadataToGitSha1Alias, FileContentMetadataToSha1Alias, FileContentMetadataToSha256Alias, FileContentToFileContentMetadata, FsnodeMappingToRootFsnode, FsnodeToChildFsnode, FsnodeToFileContent, HgBonsaiMappingToChangeset, HgChangesetToHgManifest, HgChangesetToHgParent, HgChangesetViaBonsaiToHgChangeset, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgBonsaiMapping, HgFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  Walking node types [AliasContentMapping, BonsaiHgMapping, Bookmark, Changeset, FileContent, FileContentMetadata, Fsnode, FsnodeMapping, HgBonsaiMapping, HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgFileNode, HgManifest, PhaseMapping]
  Seen,Loaded: 49,49
  * HgBonsaiMapping:3,3,0 HgChangeset:3,* HgChangesetViaBonsai:3,* HgFileEnvelope:3,*,0 HgFileNode:3,*,* HgManifest:3,3,* PhaseMapping:3,* (glob)

check the blobs were re-derived
  $ BLOBPREFIX="$TESTTMP/blobstore/blobs/blob-repo0000"
  $ BLOBCOUNT=$(ls $BLOBPREFIX.* | wc -l)
  $ echo "$BLOBCOUNT"
  36

check the sql was re-derived back to match base case
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select count(*) FROM filenodes where repo_id >= 0";
  6
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select count(*)FROM bonsai_hg_mapping where repo_id >= 0";
  3

check the base case with all the alias types present in blobstore
  $ mononoke_new_walker -L sizing scrub -q -b master_bookmark -I deep -I marker -i default -i derived_fsnodes 2>&1 | strip_glog
  Walking edge types [AliasContentMappingToFileContent, BonsaiHgMappingToHgChangesetViaBonsai, BookmarkToChangeset, ChangesetToBonsaiHgMapping, ChangesetToBonsaiParent, ChangesetToFileContent, ChangesetToFsnodeMapping, ChangesetToPhaseMapping, FileContentMetadataToGitSha1Alias, FileContentMetadataToSha1Alias, FileContentMetadataToSha256Alias, FileContentToFileContentMetadata, FsnodeMappingToRootFsnode, FsnodeToChildFsnode, FsnodeToFileContent, HgBonsaiMappingToChangeset, HgChangesetToHgManifest, HgChangesetToHgParent, HgChangesetViaBonsaiToHgChangeset, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgBonsaiMapping, HgFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  Walking node types [AliasContentMapping, BonsaiHgMapping, Bookmark, Changeset, FileContent, FileContentMetadata, Fsnode, FsnodeMapping, HgBonsaiMapping, HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgFileNode, HgManifest, PhaseMapping]
  Seen,Loaded: 49,49
  * AliasContentMapping:9,9,0 * FileContent:3,*,0 FileContentMetadata:3,0,9 * (glob)

delete the derived file metadata
  $ ls $BLOBPREFIX.* | grep -E '.(alias|content_metadata).' | xargs rm
  $ BLOBCOUNT=$(ls $BLOBPREFIX.* | wc -l)
  $ echo "$BLOBCOUNT"
  24

do a walk again, should succeed but not find the alias or other data derived from file content metadata
  $ mononoke_new_walker -L sizing scrub -q -b master_bookmark -I deep -I marker -i default -i derived_fsnodes 2>&1 | strip_glog
  Walking edge types [AliasContentMappingToFileContent, BonsaiHgMappingToHgChangesetViaBonsai, BookmarkToChangeset, ChangesetToBonsaiHgMapping, ChangesetToBonsaiParent, ChangesetToFileContent, ChangesetToFsnodeMapping, ChangesetToPhaseMapping, FileContentMetadataToGitSha1Alias, FileContentMetadataToSha1Alias, FileContentMetadataToSha256Alias, FileContentToFileContentMetadata, FsnodeMappingToRootFsnode, FsnodeToChildFsnode, FsnodeToFileContent, HgBonsaiMappingToChangeset, HgChangesetToHgManifest, HgChangesetToHgParent, HgChangesetViaBonsaiToHgChangeset, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgBonsaiMapping, HgFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  Walking node types [AliasContentMapping, BonsaiHgMapping, Bookmark, Changeset, FileContent, FileContentMetadata, Fsnode, FsnodeMapping, HgBonsaiMapping, HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgFileNode, HgManifest, PhaseMapping]
  Seen,Loaded: 40,40
  * AliasContentMapping:0,0,0 * FileContent:3,*,0 FileContentMetadata:3,0,0 * (glob)
