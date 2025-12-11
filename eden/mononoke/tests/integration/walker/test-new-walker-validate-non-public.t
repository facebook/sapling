# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config "blob_files"
  $ cd "$TESTTMP"

Create initial public commits with testtool_drawdag
  $ testtool_drawdag -R repo --print-hg-hashes <<'EOF'
  > A-B-C
  > # modify: A "A" "content A"
  > # modify: B "B" "content B"
  > # modify: C "C" "content C"
  > # bookmark: C master_bookmark
  > # author: A test
  > # author: B test
  > # author: C test
  > # message: A "A"
  > # message: B "B"
  > # message: C "C"
  > EOF
  A=* (glob)
  B=* (glob)
  C=* (glob)

Start Mononoke server
  $ start_and_wait_for_mononoke_server

Derive blame and unodes for public commits
  $ mononoke_admin derived-data -R repo derive -T blame -B master_bookmark
  $ mononoke_admin derived-data -R repo derive -T unodes -B master_bookmark
  $ mononoke_admin derived-data -R repo derive -T filenodes -B master_bookmark

validate, expecting all valid, checking marker types
  $ mononoke_walker validate -q -I deep -I marker -b master_bookmark 2>&1 | grep -vE "(Bytes|Walked)/s"
  [INFO] Walking edge types [AliasContentMappingToFileContent, BonsaiHgMappingToHgChangesetViaBonsai, BookmarkToChangeset, ChangesetToBonsaiHgMapping, ChangesetToBonsaiParent, ChangesetToFileContent, ChangesetToPhaseMapping, FileContentMetadataV2ToGitSha1Alias, FileContentMetadataV2ToSeededBlake3Alias, FileContentMetadataV2ToSha1Alias, FileContentMetadataV2ToSha256Alias, FileContentToFileContentMetadataV2, HgBonsaiMappingToChangeset, HgChangesetToHgManifest, HgChangesetToHgParent, HgChangesetViaBonsaiToHgChangeset, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgBonsaiMapping, HgFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  [INFO] Walking node types [AliasContentMapping, BonsaiHgMapping, Bookmark, Changeset, FileContent, FileContentMetadataV2, HgBonsaiMapping, HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgFileNode, HgManifest, PhaseMapping]
  [INFO] [walker validate{repo=repo}] Performing check types [ChangesetPhaseIsPublic, HgLinkNodePopulated]
  [INFO] [walker validate{repo=repo}] Seen,Loaded: 46,46
  [INFO] [walker validate{repo=repo}] Nodes,Pass,Fail:46,6,0; EdgesChecked:12; CheckType:Pass,Fail Total:6,0 ChangesetPhaseIsPublic:3,0 HgLinkNodePopulated:3,0

Remove the phase information, linknodes already point to them
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM phases where repo_id >= 0";

validate, expect no failures on phase info, as the commits are still public, just not marked as so in the phases table
  $ mononoke_walker validate -q -I deep -I marker -b master_bookmark 2>&1 | grep -vE "(Bytes|Walked)/s"
  [INFO] Walking edge types [AliasContentMappingToFileContent, BonsaiHgMappingToHgChangesetViaBonsai, BookmarkToChangeset, ChangesetToBonsaiHgMapping, ChangesetToBonsaiParent, ChangesetToFileContent, ChangesetToPhaseMapping, FileContentMetadataV2ToGitSha1Alias, FileContentMetadataV2ToSeededBlake3Alias, FileContentMetadataV2ToSha1Alias, FileContentMetadataV2ToSha256Alias, FileContentToFileContentMetadataV2, HgBonsaiMappingToChangeset, HgChangesetToHgManifest, HgChangesetToHgParent, HgChangesetViaBonsaiToHgChangeset, HgFileEnvelopeToFileContent, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgBonsaiMapping, HgFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  [INFO] Walking node types [AliasContentMapping, BonsaiHgMapping, Bookmark, Changeset, FileContent, FileContentMetadataV2, HgBonsaiMapping, HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgFileNode, HgManifest, PhaseMapping]
  [INFO] [walker validate{repo=repo}] Performing check types [ChangesetPhaseIsPublic, HgLinkNodePopulated]
  [INFO] [walker validate{repo=repo}] Seen,Loaded: 46,46
  [INFO] [walker validate{repo=repo}] Nodes,Pass,Fail:46,6,0; EdgesChecked:12; CheckType:Pass,Fail Total:6,0 ChangesetPhaseIsPublic:3,0 HgLinkNodePopulated:3,0

Record the filenode info
  $ PATHHASHC=$(sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT hex(path_hash) FROM paths WHERE path = CAST('C' as BLOB)")
  $ FILENODEC=$(sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT hex(filenode) FROM filenodes where linknode=x'$C' and path_hash=x'$PATHHASHC'")

Clone from Mononoke to create local HG repo for non-public commits
  $ cd "$TESTTMP"
  $ hg clone -q mono:repo repo_hg --noupdate
  $ cd repo_hg
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > EOF
  $ hg update -q master_bookmark

Get the public commit C hash for later use
  $ HGCOMMITC="$C"

Create a new non-public commit by pushing to temp bookmark then deleting it
  $ echo "content C modified" > C
  $ hg commit -m "C modified - non-public"
  $ HGCOMMITCNEW=$(hg log -r . -T '{node}')

Push to a temporary bookmark
  $ hg push --to temp_nonpublic --create
  pushing rev f31c6f6e8ca6 to destination mono:repo bookmark temp_nonpublic
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  exporting bookmark temp_nonpublic
  $ cd "$TESTTMP"

Delete the temporary bookmark to make the commit non-public
  $ mononoke_admin bookmarks -R repo delete temp_nonpublic
  Deleting publishing bookmark temp_nonpublic at 749154ae6c25011f8fe3765c9cb1af019413f0e9c945135c846c3cfcad212139

Delete filenodes for the non-public commit to simulate not deriving them
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM filenodes WHERE linknode=x'$HGCOMMITCNEW'"

Remove the phase information so we do not use a cached public value
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM phases where repo_id >= 0";

Update filenode for public commit C to have linknode pointing to non-public commit D
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "UPDATE filenodes SET linknode=x'$HGCOMMITCNEW' where path_hash=x'$PATHHASHC'"

Check we can walk blame on a public commit. In this walk all the Changeset history steps come from blame as we exclude ChangesetToBonsaiParent etc
  $ mononoke_walker scrub -q --walk-root=HgBonsaiMapping:${HGCOMMITC} -I deep -i bonsai -i derived_unodes -i derived_blame -i HgBonsaiMapping -X ChangesetToBonsaiParent -X UnodeFileToLinkedChangeset -X UnodeManifestToLinkedChangeset 2>&1 | grep -vE "(Bytes|Walked)/s"
  [INFO] Walking edge types [BlameToChangeset, ChangesetToUnodeMapping, HgBonsaiMappingToChangeset, UnodeFileToBlame, UnodeFileToUnodeFileParent, UnodeManifestToUnodeFileChild, UnodeManifestToUnodeManifestChild, UnodeManifestToUnodeManifestParent, UnodeMappingToRootUnodeManifest]
  [INFO] Walking node types [Blame, Changeset, HgBonsaiMapping, UnodeFile, UnodeManifest, UnodeMapping]
  [INFO] [walker scrub{repo=repo}] Seen,Loaded: 16,16

Check we dont walk blame on a non-public commit.  Because blame is the only path to Changeset history, this results in a shallow walk
  $ mononoke_walker scrub -q --walk-root=HgBonsaiMapping:${HGCOMMITCNEW} -I deep -i bonsai -i derived_unodes -i derived_blame -i HgBonsaiMapping -X ChangesetToBonsaiParent -X UnodeFileToLinkedChangeset -X UnodeManifestToLinkedChangeset 2>&1 | grep -vE "(Bytes|Walked)/s"
  [INFO] Walking edge types [BlameToChangeset, ChangesetToUnodeMapping, HgBonsaiMappingToChangeset, UnodeFileToBlame, UnodeFileToUnodeFileParent, UnodeManifestToUnodeFileChild, UnodeManifestToUnodeManifestChild, UnodeManifestToUnodeManifestParent, UnodeMappingToRootUnodeManifest]
  [INFO] Walking node types [Blame, Changeset, HgBonsaiMapping, UnodeFile, UnodeManifest, UnodeMapping]
  [INFO] [walker scrub{repo=repo}] Seen,Loaded: 3,3

Check we can walk filenodes on a public commit. In this walk all the HgChangeset history steps come from filenodes as we exclude HgChangesetToHgParent etc
  $ mononoke_walker scrub -q --walk-root=HgChangesetViaBonsai:${HGCOMMITC} -I deep -x HgBonsaiMapping -i derived_filenodes -i derived_hgchangesets -x HgManifestFileNode -X HgChangesetToHgParent 2>&1 | grep -vE "(Bytes|Walked)/s"
  [INFO] Walking edge types [HgChangesetToHgManifest, HgChangesetViaBonsaiToHgChangeset, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  [INFO] Walking node types [HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgFileNode, HgManifest]
  [INFO] [walker scrub{repo=repo}] Seen,Loaded: 20,20

Check we can walk manifest filenodes on a public commit. In this walk all the HgChangeset history steps come from mf filenodes as we exclude HgChangesetToHgParent etc
  $ mononoke_walker scrub -q --walk-root=HgChangesetViaBonsai:${HGCOMMITC} -I deep -x HgBonsaiMapping -i derived_filenodes -i derived_hgchangesets -x HgFileNode -X HgChangesetToHgParent 2>&1 | grep -vE "(Bytes|Walked)/s"
  [INFO] Walking edge types [HgChangesetToHgManifest, HgChangesetToHgManifestFileNode, HgChangesetViaBonsaiToHgChangeset, HgManifestFileNodeToHgCopyfromFileNode, HgManifestFileNodeToHgParentFileNode, HgManifestFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope]
  [INFO] Walking node types [HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgManifest, HgManifestFileNode]
  [INFO] [walker scrub{repo=repo}] Seen,Loaded: 15,15

Check we dont walk filenodes on a non-public commit.  Because filenodes is the only path to HgChangeset history, this results in a shallow walk
  $ mononoke_walker scrub -q --walk-root=HgChangeset:${HGCOMMITCNEW} -I deep -x HgBonsaiMapping -i derived_filenodes -i derived_hgchangesets -X HgChangesetToHgParent 2>&1 | grep -vE "(Bytes|Walked)/s"
  [INFO] Walking edge types [HgChangesetToHgManifest, HgChangesetToHgManifestFileNode, HgChangesetViaBonsaiToHgChangeset, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgChangeset, HgManifestFileNodeToHgCopyfromFileNode, HgManifestFileNodeToHgParentFileNode, HgManifestFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  [INFO] Walking node types [HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgFileNode, HgManifest, HgManifestFileNode]
  [INFO] [walker scrub{repo=repo}] Seen,Loaded: 16,16

validate, expect failures on phase info, and linknode as we now point to a non-public commit
  $ mononoke_walker --scuba-log-file scuba.json validate -q -I deep -I marker -b master_bookmark 2>&1 | grep 'Validation failed:' | sed 's/.*"check_type":"\([^"]*\)".*/\1/' | sort
  bonsai_phase_is_public
  hg_link_node_populated

Check scuba data
  $ wc -l < scuba.json
  2
  $ jq -r '.int * .normal | [ .check_fail, .check_type, .node_key, .node_path, .node_type, .repo, .src_node_key, .src_node_path, .src_node_type, .via_node_key, .via_node_path, .via_node_type, .walk_type ] | @csv' < scuba.json | sort
  1,"bonsai_phase_is_public","changeset.blake2.749154ae6c25011f8fe3765c9cb1af019413f0e9c945135c846c3cfcad212139",,"PhaseMapping","repo","changeset.blake2.749154ae6c25011f8fe3765c9cb1af019413f0e9c945135c846c3cfcad212139",,"Changeset","hgchangeset.sha1.0fca0879086b4ce5032340fcad666d159f4ba7e3",,"HgChangeset","validate"
  1,"hg_link_node_populated","hgfilenode.sha1.713d711e472e7b7d9fe3d7d0ee9054ab143c3a4b","C","HgFileNode","repo","hgmanifest.sha1.d73c0076ee7d9ae616321a0f3517270c9b833321","(none)","HgManifest","hgchangeset.sha1.f31c6f6e8ca6e97c836b9f5cd636430e0568fa83",,"HgChangeset","validate"
