# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ LFS_THRESHOLD=1 setup_common_config "blob_files"
  $ cd "$TESTTMP"
  $ testtool_drawdag -R repo --print-hg-hashes <<'EOF'
  > A-B-C
  > # bookmark: C master_bookmark
  > EOF
  A=20ca2a4749a439b459125ef0f6a4f26e88ee7538
  B=80521a640a0c8f51dcc128c2658b224d595840ac
  C=d3b399ca8757acdb81c3681b052eb978db6768d8

validate with LFS enabled, shallow
  $ mononoke_walker --scuba-log-file scuba-validate-shallow.json validate --include-check-type=FileContentIsLfs -I shallow -I BookmarkToBonsaiHgMapping -i hg -x HgFileNode -i FileContent -i FileContentMetadataV2 -q -b master_bookmark 2>&1 | grep -vE "(Bytes|Walked)/s"
  [INFO] Walking edge types [BonsaiHgMappingToHgChangesetViaBonsai, BookmarkToBonsaiHgMapping, FileContentToFileContentMetadataV2, HgChangesetToHgManifest, HgChangesetToHgManifestFileNode, HgChangesetViaBonsaiToHgChangeset, HgFileEnvelopeToFileContent, HgManifestToChildHgManifest, HgManifestToHgFileEnvelope, HgManifestToHgManifestFileNode]
  [INFO] Walking node types [BonsaiHgMapping, FileContent, FileContentMetadataV2, HgChangeset, HgChangesetViaBonsai, HgFileEnvelope, HgManifest, HgManifestFileNode]
  [INFO] [walker validate{repo=repo}] Performing check types [FileContentIsLfs]
  [INFO] [walker validate{repo=repo}] Seen,Loaded: 2,2
  [INFO] [walker validate{repo=repo}] Nodes,Pass,Fail:2,0,0; EdgesChecked:0; CheckType:Pass,Fail Total:0,0 FileContentIsLfs:0,0

Check scuba data is logged for lfs and that it contains useful hg changeset and path in via_node_key and node_path.  As its shallow walk expect all via_node_key to be the same
  $ wc -l < scuba-validate-shallow.json
  0
  $ jq -r '.int * .normal | [ .check_fail, .check_type, .check_size, .node_key, .node_path, .node_type, .repo, .src_node_type, .via_node_key, .via_node_type, .walk_type, .error_msg ] | @csv' < scuba-validate-shallow.json | sort

Make a commit for a file in a subdir path
  $ testtool_drawdag -R repo --print-hg-hashes <<'EOF'
  > C-D
  > # modify: D foo/bar "bar\n"
  > # bookmark: D master_bookmark
  > EOF
  C=98ad2cb5f49bd1b99ba04ffd0c3aa21d53ac557a
  D=ca790130b8c58c893b0cececac3704bf565a48bc

validate with LFS enabled, deep.  Params are setup so that ValidateRoute contains the HgChangeset that originated a Bonsai and then the Bonsai points to the files it touched.
  $ mononoke_walker --scuba-log-file scuba-validate-deep.json validate --include-check-type=FileContentIsLfs -I deep -X HgFileNodeToLinkedHgChangeset -X HgFileNodeToHgParentFileNode -X HgFileNodeToHgCopyfromFileNode -X ChangesetToBonsaiParent -X ChangesetToBonsaiHgMapping -X HgChangesetToHgParent -i default -x HgFileEnvelope -x AliasContentMapping -q -p BonsaiHgMapping 2>&1 | grep -vE "(Bytes|Walked)/s"
  [INFO] Walking edge types [BonsaiHgMappingToHgChangesetViaBonsai, ChangesetToFileContent, FileContentToFileContentMetadataV2, HgBonsaiMappingToChangeset, HgChangesetToHgManifest, HgChangesetViaBonsaiToHgChangeset, HgFileNodeToLinkedHgBonsaiMapping, HgManifestToChildHgManifest, HgManifestToHgFileNode]
  [INFO] Walking node types [BonsaiHgMapping, Changeset, FileContent, FileContentMetadataV2, HgBonsaiMapping, HgChangeset, HgChangesetViaBonsai, HgFileNode, HgManifest]
  [INFO] [walker validate{repo=repo}] Performing check types [FileContentIsLfs]
  [INFO] [walker validate{repo=repo}] Repo bounds: (1, 6)
  [INFO] [walker validate{repo=repo}] Starting chunk 1 with bounds (1, 6)
  [INFO] [walker validate{repo=repo}] Seen,Loaded: 5,5
  [INFO] [walker validate{repo=repo}] Nodes,Pass,Fail:5,0,0; EdgesChecked:0; CheckType:Pass,Fail Total:0,0 FileContentIsLfs:0,0
  [INFO] [walker validate{repo=repo}] Deferred: 0
  [INFO] [walker validate{repo=repo}] Completed in 1 chunks of size 100000

Check scuba data is logged for lfs and that it contains useful hg changeset and path in via_node_key and node_path
  $ wc -l < scuba-validate-deep.json
  0
  $ jq -r '.int * .normal | [ .check_fail, .check_type, .node_key, .node_path, .node_type, .repo, .src_node_type, .via_node_key, .via_node_type, .walk_type, .error_msg ] | @csv' < scuba-validate-deep.json | sort


validate with LFS enabled, deep with simpler query.  Should have same output but touch less nodes to get there.
  $ mononoke_walker --scuba-log-file scuba-validate-deep2.json validate --include-check-type=FileContentIsLfs -I deep -I BonsaiHgMappingToHgBonsaiMapping -X BonsaiHgMappingToHgChangesetViaBonsai -X ChangesetToBonsaiParent -X ChangesetToBonsaiHgMapping -i bonsai -i FileContent -i FileContentMetadataV2 -i HgBonsaiMapping -i BonsaiHgMapping -q -p BonsaiHgMapping 2>&1 | grep -vE "(Bytes|Walked)/s"
  [INFO] Walking edge types [BonsaiHgMappingToHgBonsaiMapping, ChangesetToFileContent, FileContentToFileContentMetadataV2, HgBonsaiMappingToChangeset]
  [INFO] Walking node types [BonsaiHgMapping, Changeset, FileContent, FileContentMetadataV2, HgBonsaiMapping]
  [INFO] [walker validate{repo=repo}] Performing check types [FileContentIsLfs]
  [INFO] [walker validate{repo=repo}] Repo bounds: (1, 6)
  [INFO] [walker validate{repo=repo}] Starting chunk 1 with bounds (1, 6)
  [INFO] [walker validate{repo=repo}] Seen,Loaded: 25,25
  [INFO] [walker validate{repo=repo}] Nodes,Pass,Fail:25,5,0; EdgesChecked:5; CheckType:Pass,Fail Total:5,0 FileContentIsLfs:5,0
  [INFO] [walker validate{repo=repo}] Deferred: 0
  [INFO] [walker validate{repo=repo}] Completed in 1 chunks of size 100000

Check scuba data is logged for lfs and that it contains useful hg changeset and path in via_node_key and node_path
  $ wc -l < scuba-validate-deep2.json
  5
  $ jq -r '.int * .normal | [ .check_fail, .check_type, .node_key, .node_path, .node_type, .repo, .src_node_type, .via_node_key, .via_node_type, .walk_type, .error_msg ] | @csv' < scuba-validate-deep2.json | sort
  0,"file_content_is_lfs","content.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f","B","FileContentMetadataV2","repo","Changeset","hgchangeset.sha1.80521a640a0c8f51dcc128c2658b224d595840ac","HgBonsaiMapping","validate",
  0,"file_content_is_lfs","content.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d","C","FileContentMetadataV2","repo","Changeset","hgchangeset.sha1.98ad2cb5f49bd1b99ba04ffd0c3aa21d53ac557a","HgBonsaiMapping","validate",
  0,"file_content_is_lfs","content.blake2.90c8e211c758a9bbcd33e463c174f1693692677cb76c7aaf4ce41aa0a29334c0","D","FileContentMetadataV2","repo","Changeset","hgchangeset.sha1.ca790130b8c58c893b0cececac3704bf565a48bc","HgBonsaiMapping","validate",
  0,"file_content_is_lfs","content.blake2.e164fd53a3714f754d5f5763688bea02d99123436e51e9ed9c85ad04fdc52222","foo/bar","FileContentMetadataV2","repo","Changeset","hgchangeset.sha1.ca790130b8c58c893b0cececac3704bf565a48bc","HgBonsaiMapping","validate",
  0,"file_content_is_lfs","content.blake2.eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9","A","FileContentMetadataV2","repo","Changeset","hgchangeset.sha1.20ca2a4749a439b459125ef0f6a4f26e88ee7538","HgBonsaiMapping","validate",
