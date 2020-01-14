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

validate, expecting all valid, checking marker types
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
  $ blobimport repo-hg/.hg repo --no-bookmark

Remove the phase information so we don't use a cached Public value
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM phases where repo_id >= 0";

Update filenode for public commit C to have linknode pointing to non-public commit D
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "UPDATE filenodes SET linknode=x'$HGCOMMITCNEW' where path_hash=x'$PATHHASHC'"

validate, expect failures on phase info, as we now point to a non-public commit
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore validate -I deep -I marker -q --bookmark master_bookmark 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Performing check types [BonsaiChangesetPhaseIsPublic, HgLinkNodePopulated]
  Validation failed: *bonsai_phase_is_public* (glob)
  Final count: * (glob)
  Walked* (glob)
  Nodes,Pass,Fail:52,7,1; EdgesChecked:16; CheckType:Pass,Fail Total:7,1 BonsaiChangesetPhaseIsPublic:3,1 HgLinkNodePopulated:4,0
