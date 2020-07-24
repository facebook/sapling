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

validate, expecting all valid, checking marker types
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore validate -I deep -I marker -q --bookmark master_bookmark 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Performing check types [BonsaiChangesetPhaseIsPublic, HgLinkNodePopulated]
  Final count: * (glob)
  Walked* (glob)
  Nodes,Pass,Fail:43,6,0; EdgesChecked:12; CheckType:Pass,Fail Total:6,0 BonsaiChangesetPhaseIsPublic:3,0 HgLinkNodePopulated:3,0

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
  Nodes,Pass,Fail:43,6,0; EdgesChecked:12; CheckType:Pass,Fail Total:6,0 BonsaiChangesetPhaseIsPublic:3,0 HgLinkNodePopulated:3,0

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

Remove the phase information so we do not use a cached public value
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM phases where repo_id >= 0";

Update filenode for public commit C to have linknode pointing to non-public commit D
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "UPDATE filenodes SET linknode=x'$HGCOMMITCNEW' where path_hash=x'$PATHHASHC'"

validate, expect failures on phase info, as we now point to a non-public commit
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore validate --scuba-log-file scuba.json -I deep -I marker -q --bookmark master_bookmark 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Performing check types [BonsaiChangesetPhaseIsPublic, HgLinkNodePopulated]
  Validation failed: *bonsai_phase_is_public* (glob)
  Final count: * (glob)
  Walked* (glob)
  Nodes,Pass,Fail:56,7,1; EdgesChecked:16; CheckType:Pass,Fail Total:7,1 BonsaiChangesetPhaseIsPublic:3,1 HgLinkNodePopulated:4,0

Check scuba data
  $ count_stdin_lines < scuba.json
  1
  $ jq -r '.int * .normal | [ .check_fail, .check_type, .node_key, .node_path, .node_type, .repo, .src_node_key, .src_node_path, .src_node_type, .via_node_key, .via_node_path, .via_node_type, .walk_type ] | @csv' < scuba.json | sort
  1,"bonsai_phase_is_public","changeset.blake2.2b06a8547bfe6a3ac79392aef3fa7f3f45a82f4e0beb95c4fa2b914c34b5b215",,"BonsaiPhaseMapping","repo","changeset.blake2.2b06a8547bfe6a3ac79392aef3fa7f3f45a82f4e0beb95c4fa2b914c34b5b215",,"BonsaiChangeset","hgchangeset.sha1.26805aba1e600a82e93661149f2313866a221a7b",,"HgChangeset","validate"
