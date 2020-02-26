# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ default_setup_blobimport "blob_files"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  |
  o  B [draft;rev=1;112478962961]
  |
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting

validate, expecting all valid
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore validate -I deep -q --bookmark master_bookmark 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Performing check types [HgLinkNodePopulated]
  Final count: * (glob)
  Walked* (glob)
  Nodes,Pass,Fail:37,3,0; EdgesChecked:9; CheckType:Pass,Fail Total:3,0 HgLinkNodePopulated:3,0

Remove all filenodes
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM filenodes where linknode=x'112478962961147124EDD43549AEDD1A335E44BF'";

validate, expecting validation fails
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore validate -I deep -q --bookmark master_bookmark --scuba-log-file scuba.json 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Performing check types [HgLinkNodePopulated]
  Validation failed: *hg_link_node_populated* (glob)
  Final count: * (glob)
  Walked* (glob)
  Nodes,Pass,Fail:36,2,1; EdgesChecked:7; CheckType:Pass,Fail Total:2,1 HgLinkNodePopulated:2,1

Check scuba data
  $ wc -l < scuba.json
  1
  $ jq -r '.int * .normal | [ .check_fail, .check_type, .node_key, .node_path, .node_type, .repo, .walk_type ] | @csv' < scuba.json | sort
  1,"hg_link_node_populated","hgfilenode.sha1.35e7525ce3a48913275d7061dd9a867ffef1e34d","B","HgFileNode","repo","validate"

repair by blobimport.
  $ blobimport repo-hg/.hg repo

validate, expecting all valid, this time checking marker types as well
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore validate -I deep -I marker -q --bookmark master_bookmark 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Performing check types [BonsaiChangesetPhaseIsPublic, HgLinkNodePopulated]
  Final count: * (glob)
  Walked* (glob)
  Nodes,Pass,Fail:40,6,0; EdgesChecked:12; CheckType:Pass,Fail Total:6,0 BonsaiChangesetPhaseIsPublic:3,0 HgLinkNodePopulated:3,0

Remove the phase information, linknodes already point to them
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM phases where repo_id >= 0";

validate, expect no failures on phase info, as the commits are still public, just not marked as so in the phases table
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore validate -I deep -I marker -q --bookmark master_bookmark 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Performing check types [BonsaiChangesetPhaseIsPublic, HgLinkNodePopulated]
  Final count: * (glob)
  Walked* (glob)
  Nodes,Pass,Fail:40,6,0; EdgesChecked:12; CheckType:Pass,Fail Total:6,0 BonsaiChangesetPhaseIsPublic:3,0 HgLinkNodePopulated:3,0

Remove all filenodes for the last commit, validation should succeed (i.e. filenodes were not derived yet)
  $ cd "$TESTTMP"
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM filenodes where HEX(linknode) like '26805aba1e600a82e93661149f2313866a221a7b'";
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore validate -I deep -q --bookmark master_bookmark 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Performing check types [HgLinkNodePopulated]
  Final count: * (glob)
  Walked* (glob)
  Nodes,Pass,Fail:32,2,0; EdgesChecked:6; CheckType:Pass,Fail Total:2,0 HgLinkNodePopulated:2,0
