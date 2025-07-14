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

  $ hginit_treemanifest repo
  $ cd repo || exit 1
  $ drawdag <<EOF
  > C
  > |
  > B
  > |
  > A
  > EOF

  $ hg bookmark master_bookmark -r tip
  $ cd ..
  $ blobimport repo/.hg repo --derived-data-type fsnodes

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
  33

Do a walk of everything, it should all be there
  $ mononoke_walker -L sizing scrub -q -b master_bookmark -I deep -I marker -i default -i derived_fsnodes 2>&1 | grep -vE "(Bytes|Walked)/s"
  [INFO] Walking edge types [AliasContentMappingToFileContent, BonsaiHgMappingToHgChangesetViaBonsai, BookmarkToChangeset, ChangesetToBonsaiHgMapping, ChangesetToBonsaiParent, ChangesetToFileContent, ChangesetToFsnodeMapping, ChangesetToPhaseMapping, FileContentMetadataV2ToGitSha1Alias, FileContentMetadataV2ToSeededBlake3Alias, FileContentMetadataV2ToSha1Alias, FileContentMetadataV2ToSha256Alias, FileContentToFileContentMetadataV2, FsnodeMappingToRootFsnode, FsnodeToChildFsnode, FsnodeToFileContent, HgBonsaiMappingToChangeset, HgChangesetToHgManifest, HgChangesetToHgParent, HgChangesetViaBonsaiToHgChangeset, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgBonsaiMapping, HgFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  [INFO] Walking node types [AliasContentMapping, BonsaiHgMapping, Bookmark, Changeset, FileContent, FileContentMetadataV2, Fsnode, FsnodeMapping, HgBonsaiMapping, HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgFileNode, HgManifest, PhaseMapping]
  [INFO] [walker scrub{repo=repo}] Seen,Loaded: 49,49

check the metadata base case
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select count(*) FROM filenodes where repo_id >= 0";
  6
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select count(*)FROM bonsai_hg_mapping where repo_id >= 0";
  3

delete the hg blob forms
  $ ls $BLOBPREFIX.* | grep -E '.(filenode_lookup|hgchangeset|hgfilenode|hgmanifest).' | xargs rm
  $ BLOBCOUNT=$(ls $BLOBPREFIX.* | wc -l)
  $ echo "$BLOBCOUNT"
  21

delete the hg db forms
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM filenodes where repo_id >= 0";
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM bonsai_hg_mapping where repo_id >= 0";

Do a walk again, should succeed but not find the hg types
  $ mononoke_walker -L sizing scrub -q -b master_bookmark -I deep -I marker -i default -i derived_fsnodes 2>&1 | grep -vE "(Bytes|Walked)/s"
  [INFO] Walking edge types [AliasContentMappingToFileContent, BonsaiHgMappingToHgChangesetViaBonsai, BookmarkToChangeset, ChangesetToBonsaiHgMapping, ChangesetToBonsaiParent, ChangesetToFileContent, ChangesetToFsnodeMapping, ChangesetToPhaseMapping, FileContentMetadataV2ToGitSha1Alias, FileContentMetadataV2ToSeededBlake3Alias, FileContentMetadataV2ToSha1Alias, FileContentMetadataV2ToSha256Alias, FileContentToFileContentMetadataV2, FsnodeMappingToRootFsnode, FsnodeToChildFsnode, FsnodeToFileContent, HgBonsaiMappingToChangeset, HgChangesetToHgManifest, HgChangesetToHgParent, HgChangesetViaBonsaiToHgChangeset, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgBonsaiMapping, HgFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  [INFO] Walking node types [AliasContentMapping, BonsaiHgMapping, Bookmark, Changeset, FileContent, FileContentMetadataV2, Fsnode, FsnodeMapping, HgBonsaiMapping, HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgFileNode, HgManifest, PhaseMapping]
  [INFO] [walker scrub{repo=repo}] Seen,Loaded: 31,31

Do a walk again, with --enable-derive, should succeed with the full count
  $ mononoke_walker --with-readonly-storage=false -L sizing scrub -q --enable-derive -b master_bookmark -I deep -I marker -i default -i derived_fsnodes 2>&1 | grep -vE "(Bytes|Walked)/s"
  [INFO] Walking edge types [AliasContentMappingToFileContent, BonsaiHgMappingToHgChangesetViaBonsai, BookmarkToChangeset, ChangesetToBonsaiHgMapping, ChangesetToBonsaiParent, ChangesetToFileContent, ChangesetToFsnodeMapping, ChangesetToPhaseMapping, FileContentMetadataV2ToGitSha1Alias, FileContentMetadataV2ToSeededBlake3Alias, FileContentMetadataV2ToSha1Alias, FileContentMetadataV2ToSha256Alias, FileContentToFileContentMetadataV2, FsnodeMappingToRootFsnode, FsnodeToChildFsnode, FsnodeToFileContent, HgBonsaiMappingToChangeset, HgChangesetToHgManifest, HgChangesetToHgParent, HgChangesetViaBonsaiToHgChangeset, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgBonsaiMapping, HgFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  [INFO] Walking node types [AliasContentMapping, BonsaiHgMapping, Bookmark, Changeset, FileContent, FileContentMetadataV2, Fsnode, FsnodeMapping, HgBonsaiMapping, HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgFileNode, HgManifest, PhaseMapping]
  [INFO] [walker scrub{repo=repo}] Seen,Loaded: 52,52

check the blobs were re-derived
  $ BLOBPREFIX="$TESTTMP/blobstore/blobs/blob-repo0000"
  $ BLOBCOUNT=$(ls $BLOBPREFIX.* | wc -l)
  $ echo "$BLOBCOUNT"
  39

check the sql was re-derived back to match base case
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select count(*) FROM filenodes where repo_id >= 0";
  6
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select count(*)FROM bonsai_hg_mapping where repo_id >= 0";
  3

check the base case with all the alias types present in blobstore
  $ mononoke_walker -L sizing scrub -q -b master_bookmark -I deep -I marker -i default -i derived_fsnodes 2>&1 | grep -vE "(Bytes|Walked)/s"
  [INFO] Walking edge types [AliasContentMappingToFileContent, BonsaiHgMappingToHgChangesetViaBonsai, BookmarkToChangeset, ChangesetToBonsaiHgMapping, ChangesetToBonsaiParent, ChangesetToFileContent, ChangesetToFsnodeMapping, ChangesetToPhaseMapping, FileContentMetadataV2ToGitSha1Alias, FileContentMetadataV2ToSeededBlake3Alias, FileContentMetadataV2ToSha1Alias, FileContentMetadataV2ToSha256Alias, FileContentToFileContentMetadataV2, FsnodeMappingToRootFsnode, FsnodeToChildFsnode, FsnodeToFileContent, HgBonsaiMappingToChangeset, HgChangesetToHgManifest, HgChangesetToHgParent, HgChangesetViaBonsaiToHgChangeset, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgBonsaiMapping, HgFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  [INFO] Walking node types [AliasContentMapping, BonsaiHgMapping, Bookmark, Changeset, FileContent, FileContentMetadataV2, Fsnode, FsnodeMapping, HgBonsaiMapping, HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgFileNode, HgManifest, PhaseMapping]
  [INFO] [walker scrub{repo=repo}] Seen,Loaded: 52,52
