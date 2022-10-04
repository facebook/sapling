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
  $ blobimport repo-hg/.hg repo --derived-data-type=blame --derived-data-type=unodes

validate, expecting all valid, checking marker types
  $ mononoke_walker -l validate validate -q -I deep -I marker -b master_bookmark 2>&1 | strip_glog
  Performing check types [ChangesetPhaseIsPublic, HgLinkNodePopulated]
  Nodes,Pass,Fail:43,6,0; EdgesChecked:12; CheckType:Pass,Fail Total:6,0 ChangesetPhaseIsPublic:3,0 HgLinkNodePopulated:3,0

Remove the phase information, linknodes already point to them
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM phases where repo_id >= 0";

validate, expect no failures on phase info, as the commits are still public, just not marked as so in the phases table
  $ mononoke_walker -l validate validate -q -I deep -I marker -b master_bookmark 2>&1 | strip_glog
  Performing check types [ChangesetPhaseIsPublic, HgLinkNodePopulated]
  Nodes,Pass,Fail:43,6,0; EdgesChecked:12; CheckType:Pass,Fail Total:6,0 ChangesetPhaseIsPublic:3,0 HgLinkNodePopulated:3,0

Record the filenode info
  $ PATHHASHC=$(sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT hex(path_hash) FROM paths WHERE path = CAST('C' as BLOB)")
  $ FILENODEC=$(sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT hex(filenode) FROM filenodes where linknode=x'$HGCOMMITC' and path_hash=x'$PATHHASHC'")

Make a really non-public commit by importing it and not advancing bookmarks
  $ BONSAIPUBLIC=$(get_bonsai_bookmark $REPOID master_bookmark)
  $ cd repo-hg
  $ HGCOMMITC=$(hg log -r tip -T '{node}')
  $ mkcommit C
  $ HGCOMMITCNEW=$(hg log -r tip -T '{node}')
  $ cd ..
  $ blobimport repo-hg/.hg repo --no-bookmark --derived-data-type=unodes --exclude-derived-data-type=filenodes

Remove the phase information so we do not use a cached public value
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM phases where repo_id >= 0";

Update filenode for public commit C to have linknode pointing to non-public commit D
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "UPDATE filenodes SET linknode=x'$HGCOMMITCNEW' where path_hash=x'$PATHHASHC'"

Check we can walk blame on a public commit. In this walk all the Changeset history steps come from blame as we exclude ChangesetToBonsaiParent etc
  $ mononoke_walker -L sizing scrub -q --walk-root=HgBonsaiMapping:${HGCOMMITC} -I deep -i bonsai -i derived_unodes -i derived_blame -i HgBonsaiMapping -X ChangesetToBonsaiParent -X UnodeFileToLinkedChangeset -X UnodeManifestToLinkedChangeset 2>&1 | strip_glog
  Walking edge types [BlameToChangeset, ChangesetToUnodeMapping, HgBonsaiMappingToChangeset, UnodeFileToBlame, UnodeFileToUnodeFileParent, UnodeManifestToUnodeFileChild, UnodeManifestToUnodeManifestChild, UnodeManifestToUnodeManifestParent, UnodeMappingToRootUnodeManifest]
  Walking node types [Blame, Changeset, HgBonsaiMapping, UnodeFile, UnodeManifest, UnodeMapping]
  Seen,Loaded: 16,16
  * Type:Walked,Checks,Children Blame:3,* Changeset:3,* HgBonsaiMapping:1,* UnodeFile:3,* UnodeManifest:3,* UnodeMapping:3,* (glob)

Check we dont walk blame on a non-public commit.  Because blame is the only path to Changeset history, this results in a shallow walk
  $ mononoke_walker -L sizing scrub -q --walk-root=HgBonsaiMapping:${HGCOMMITCNEW} -I deep -i bonsai -i derived_unodes -i derived_blame -i HgBonsaiMapping -X ChangesetToBonsaiParent -X UnodeFileToLinkedChangeset -X UnodeManifestToLinkedChangeset 2>&1 | strip_glog
  Walking edge types [BlameToChangeset, ChangesetToUnodeMapping, HgBonsaiMappingToChangeset, UnodeFileToBlame, UnodeFileToUnodeFileParent, UnodeManifestToUnodeFileChild, UnodeManifestToUnodeManifestChild, UnodeManifestToUnodeManifestParent, UnodeMappingToRootUnodeManifest]
  Walking node types [Blame, Changeset, HgBonsaiMapping, UnodeFile, UnodeManifest, UnodeMapping]
  Seen,Loaded: 5,5
  * Type:Walked,Checks,Children Blame:0,* Changeset:1,* HgBonsaiMapping:1,* UnodeFile:1,* UnodeManifest:1,* UnodeMapping:1,* (glob)

Check we can walk filenodes on a public commit. In this walk all the HgChangeset history steps come from filenodes as we exclude HgChangesetToHgParent etc
  $ mononoke_walker -L sizing scrub -q --walk-root=HgChangesetViaBonsai:${HGCOMMITC} -I deep -x HgBonsaiMapping -i derived_filenodes -i derived_hgchangesets -x HgManifestFileNode -X HgChangesetToHgParent 2>&1 | strip_glog
  Walking edge types [HgChangesetToHgManifest, HgChangesetViaBonsaiToHgChangeset, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  Walking node types [HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgFileNode, HgManifest]
  Seen,Loaded: 20,20
  * Type:Walked,Checks,Children HgChangeset:4,* HgChangesetViaBonsai:4,* HgFileEnvelope:4,* HgFileNode:4,* HgManifest:4,* (glob)

Check we can walk manifest filenodes on a public commit. In this walk all the HgChangeset history steps come from mf filenodes as we exclude HgChangesetToHgParent etc
  $ mononoke_walker -L sizing scrub -q --walk-root=HgChangesetViaBonsai:${HGCOMMITC} -I deep -x HgBonsaiMapping -i derived_filenodes -i derived_hgchangesets -x HgFileNode -X HgChangesetToHgParent 2>&1 | strip_glog
  Walking edge types [HgChangesetToHgManifest, HgChangesetToHgManifestFileNode, HgChangesetViaBonsaiToHgChangeset, HgManifestFileNodeToHgCopyfromFileNode, HgManifestFileNodeToHgParentFileNode, HgManifestFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope]
  Walking node types [HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgManifest, HgManifestFileNode]
  Seen,Loaded: 15,15
  * Type:Walked,Checks,Children HgChangeset:3,* HgChangesetViaBonsai:3,* HgFileEnvelope:3,* HgManifest:3,* HgManifestFileNode:3,* (glob)

Check we dont walk filenodes on a non-public commit.  Because filenodes is the only path to HgChangeset history, this results in a shallow walk
  $ mononoke_walker -L sizing scrub -q --walk-root=HgChangeset:${HGCOMMITCNEW} -I deep -x HgBonsaiMapping -i derived_filenodes -i derived_hgchangesets -X HgChangesetToHgParent 2>&1 | strip_glog
  Walking edge types [HgChangesetToHgManifest, HgChangesetToHgManifestFileNode, HgChangesetViaBonsaiToHgChangeset, HgFileNodeToHgCopyfromFileNode, HgFileNodeToHgParentFileNode, HgFileNodeToLinkedHgChangeset, HgManifestFileNodeToHgCopyfromFileNode, HgManifestFileNodeToHgParentFileNode, HgManifestFileNodeToLinkedHgChangeset, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgFileNode]
  Walking node types [HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgFileNode, HgManifest, HgManifestFileNode]
  Seen,Loaded: 4,4
  * Type:Walked,Checks,Children HgChangeset:1,* HgFileEnvelope:1,* HgFileNode:1,* HgManifest:1,* (glob)

validate, expect failures on phase info, and linknode as we now point to a non-public commit
  $ mononoke_walker --scuba-dataset file://scuba.json -l validate validate -q -I deep -I marker -b master_bookmark 2>&1 | strip_glog  | grep 'Validation failed:' | sed 's/.*"check_type":"\([^"]*\)".*/\1/' | sort
  bonsai_phase_is_public
  hg_link_node_populated

Check scuba data
  $ wc -l < scuba.json
  2
  $ jq -r '.int * .normal | [ .check_fail, .check_type, .node_key, .node_path, .node_type, .repo, .src_node_key, .src_node_path, .src_node_type, .via_node_key, .via_node_path, .via_node_type, .walk_type ] | @csv' < scuba.json | sort
  1,"bonsai_phase_is_public","changeset.blake2.2b06a8547bfe6a3ac79392aef3fa7f3f45a82f4e0beb95c4fa2b914c34b5b215",,"PhaseMapping","repo","changeset.blake2.2b06a8547bfe6a3ac79392aef3fa7f3f45a82f4e0beb95c4fa2b914c34b5b215",,"Changeset","hgchangeset.sha1.26805aba1e600a82e93661149f2313866a221a7b",,"HgChangeset","validate"
  1,"hg_link_node_populated","hgfilenode.sha1.a57fcc2e5e0f83e36500e99f4e8d3cf03658864a","C","HgFileNode","repo","hgmanifest.sha1.40106725c7775e677bc2e84242d614a02bcbbc61","(none)","HgManifest","hgchangeset.sha1.fb2089ef1d47e570d0453428a0b5d8b5463cf9e3",,"HgChangeset","validate"
