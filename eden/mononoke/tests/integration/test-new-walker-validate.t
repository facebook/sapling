# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ default_setup_blobimport "blob_files"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting

validate, expecting all valid
  $ mononoke_walker validate -I deep -q -b master_bookmark 2>&1 | strip_glog
  Walking edge types * (glob)
  Walking node types * (glob)
  Performing check types [HgLinkNodePopulated]
  Seen,Loaded: * (glob)
  Walked* (glob)
  Nodes,Pass,Fail:40,3,0; EdgesChecked:9; CheckType:Pass,Fail Total:3,0 HgLinkNodePopulated:3,0


validate, check route is logged on unexpected error (forced with chaos blob)
  $ mononoke_walker --scuba-log-file scuba-error.json --blobstore-read-chaos-rate=1 --blobstore-cachelib-only -l validate validate -q -I deep -b master_bookmark 2>&1 | strip_glog
  Performing check types [HgLinkNodePopulated]
  Execution error: Could not step to OutgoingEdge { label: BookmarkToChangeset, target: Changeset(ChangesetKey { inner: ChangesetId(Blake2(c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd)), filenode_known_derived: false }), path: None } via Some(ValidateRoute { src_node: Bookmark(BookmarkName { bookmark: "master_bookmark" }), via: [] }) in repo repo
  
  Caused by:
      Injected failure in get to ChaosBlobstore for key repo0000.changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd
  Error: Execution failed

Check scuba data is logged for error on step and that it contains message and route info
  $ wc -l < scuba-error.json
  1
  $ jq -r '.int * .normal | [ .check_fail, .check_type, .node_key, .node_type, .repo, .src_node_type, .via_node_type, .walk_type, .error_msg ] | @csv' < scuba-error.json
  1,"step","changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd","Changeset","repo","Bookmark",,"validate","Could not step to OutgoingEdge { label: BookmarkToChangeset, target: Changeset(ChangesetKey { inner: ChangesetId(Blake2(c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd)), filenode_known_derived: false }), path: None }, due to Other(Injected failure in get to ChaosBlobstore for key repo0000.changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd), via Some(ValidateRoute { src_node: Bookmark(BookmarkName { bookmark: ""master_bookmark"" }), via: [] })"

Remove all filenodes
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM filenodes where linknode=x'112478962961147124EDD43549AEDD1A335E44BF'";

validate, expecting validation fails
  $ mononoke_walker --scuba-log-file scuba.json -l validate validate -q -I deep -b master_bookmark 2>&1 | strip_glog
  Performing check types [HgLinkNodePopulated]
  Validation failed: *hg_link_node_populated* (glob)
  Nodes,Pass,Fail:39,2,1; EdgesChecked:7; CheckType:Pass,Fail Total:2,1 HgLinkNodePopulated:2,1

Check scuba data
  $ wc -l < scuba.json
  1
  $ jq -r '.int * .normal | [ .check_fail, .check_type, .node_key, .node_path, .node_type, .repo, .src_node_key, .src_node_path, .src_node_type, .via_node_key, .via_node_path, .via_node_type, .walk_type ] | @csv' < scuba.json | sort
  1,"hg_link_node_populated","hgfilenode.sha1.35e7525ce3a48913275d7061dd9a867ffef1e34d","B","HgFileNode","repo","hgmanifest.sha1.*","(none)","HgManifest","hgchangeset.sha1.*",,"HgChangeset","validate" (glob)

repair by blobimport.
  $ blobimport repo-hg/.hg repo

validate, expecting all valid, this time checking marker types as well
  $ mononoke_walker -l validate validate -q -I deep -I marker -b master_bookmark 2>&1 | strip_glog
  Performing check types [ChangesetPhaseIsPublic, HgLinkNodePopulated]
  Nodes,Pass,Fail:43,6,0; EdgesChecked:12; CheckType:Pass,Fail Total:6,0 ChangesetPhaseIsPublic:3,0 HgLinkNodePopulated:3,0

Remove the phase information, linknodes already point to them
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM phases where repo_id >= 0";

validate, expect no failures on phase info, as the commits are still public, just not marked as so in the phases table
  $ mononoke_walker -l validate validate -q -I deep -I marker -b master_bookmark 2>&1 | strip_glog
  Performing check types [ChangesetPhaseIsPublic, HgLinkNodePopulated]
  Nodes,Pass,Fail:43,6,0; EdgesChecked:12; CheckType:Pass,Fail Total:6,0 ChangesetPhaseIsPublic:3,0 HgLinkNodePopulated:3,0

Remove all filenodes for the last commit, validation should succeed (i.e. filenodes were not derived yet)
  $ cd "$TESTTMP"
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM filenodes where HEX(linknode) like '26805aba1e600a82e93661149f2313866a221a7b'";
  $ mononoke_walker -l validate validate -q -I deep -b master_bookmark 2>&1 | strip_glog
  Performing check types [HgLinkNodePopulated]
  Nodes,Pass,Fail:34,2,0; EdgesChecked:6; CheckType:Pass,Fail Total:2,0 HgLinkNodePopulated:2,0
