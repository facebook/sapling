  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ REPOTYPE="blob_files"
  $ setup_common_config "$REPOTYPE"
  $ cd $TESTTMP

setup common configuration
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > EOF

setup repo
  $ hg init repo-hg
  $ cd repo-hg
  $ setup_hg_server
  $ hg debugdrawdag <<EOF
  > C
  > |
  > B
  > |
  > A
  > EOF

create master bookmark
  $ hg bookmark master_bookmark -r tip

blobimport, succeeding
  $ cd ..
  $ blobimport repo-hg/.hg repo

validate, expecting all valid
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore validate -I deep -q --bookmark master_bookmark 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Performing check types [HgLinkNodePopulated]
  Final count: * (glob)
  Walked* (glob)
  Nodes,Pass,Fail:28,3,0; EdgesChecked:9; CheckType:Pass,Fail Total:3,0 HgLinkNodePopulated:3,0
  Exiting...

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
  Nodes,Pass,Fail:25,0,3; EdgesChecked:3; CheckType:Pass,Fail Total:0,3 HgLinkNodePopulated:0,3
  Exiting...

repair by blobimport.
  $ blobimport repo-hg/.hg repo

validate, expecting all valid
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore validate -q --bookmark master_bookmark 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Performing check types [HgLinkNodePopulated]
  Final count: * (glob)
  Walked* (glob)
  Nodes,Pass,Fail:28,3,0; EdgesChecked:9; CheckType:Pass,Fail Total:3,0 HgLinkNodePopulated:3,0
  Exiting...

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
  Nodes,Pass,Fail:31,6,0; EdgesChecked:12; CheckType:Pass,Fail Total:6,0 BonsaiChangesetPhaseIsPublic:3,0 HgLinkNodePopulated:3,0
  Exiting...

Set commits non-public, linknodes already point to them
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM phases where repo_id >= 0";

validate, expecting failures on phase info
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore validate -I deep -I marker -q --bookmark master_bookmark 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Performing check types [BonsaiChangesetPhaseIsPublic, HgLinkNodePopulated]
  Validation failed: *bonsai_phase_is_public* (glob)
  Validation failed: *bonsai_phase_is_public* (glob)
  Validation failed: *bonsai_phase_is_public* (glob)
  Final count: * (glob)
  Walked* (glob)
  Nodes,Pass,Fail:31,3,3; EdgesChecked:12; CheckType:Pass,Fail Total:3,3 BonsaiChangesetPhaseIsPublic:0,3 HgLinkNodePopulated:3,0
  Exiting...
