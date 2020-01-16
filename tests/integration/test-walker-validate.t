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
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM filenodes where repo_id >= 0";

validate, expecting validation fails
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore validate -I deep -q --bookmark master_bookmark 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Performing check types [HgLinkNodePopulated]
  Validation failed: *hg_link_node_populated* (glob)
  Validation failed: *hg_link_node_populated* (glob)
  Validation failed: *hg_link_node_populated* (glob)
  Final count: * (glob)
  Walked* (glob)
  Nodes,Pass,Fail:34,0,3; EdgesChecked:3; CheckType:Pass,Fail Total:0,3 HgLinkNodePopulated:0,3

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
