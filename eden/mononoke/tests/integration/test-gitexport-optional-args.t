# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

Setting up a simple scenario for the gitexport tool
  $ . "${TEST_FIXTURES}/library.sh"


Setup configuration
  $ REPOTYPE="blob_files"
  $ setup_common_config "$REPOTYPE"
  $ ENABLE_API_WRITES=1 REPOID=1 setup_mononoke_repo_config "temp_repo"
  $ cd $TESTTMP


Set some env vars that will be used frequently

  $ EXPORT_DIR="export_dir"
  $ EXPORT_SUBDIR="$EXPORT_DIR/subdir_to_export"
-- Folder that should NOT be exported to the git repo
  $ INTERNAL_DIR="internal_dir"


# -------------------------- Create commits --------------------------
  $ testtool_drawdag -R repo --derive-all --no-default-files <<EOF
  > A-B-C-D
  > # modify: A "$EXPORT_DIR/B.txt" "File to export"
  > # message: A "Add files to export dir"
  > # modify: B "$INTERNAL_DIR/internal.txt" "Internal file"
  > # message: B "Add file to internal_dir"
  > # modify: C "$EXPORT_SUBDIR/export_file_in_subdir.txt" "File in export subdirectory"
  > # message: C "Add subdirectory to export dir"
  > # modify: D "$EXPORT_SUBDIR/second_subdir_export.txt" "File in export subdirectory"
  > # modify: D "$EXPORT_DIR/C.txt" "File to export"
  > # modify: D "$INTERNAL_DIR/another_internal.txt" "Internal file"
  > # message: D "Add files to all directories"
  > # author_date: A "2016-01-01T01:00:00+00:00"
  > # author_date: B "2016-01-01T02:00:00+00:00"
  > # author_date: C "2016-01-01T03:00:00+00:00"
  > # author_date: D "2016-01-01T04:00:00+00:00"
  > # bookmark: D master
  > EOF
  A=69f4d052996dc4a3fba7ab86939f567ad5a9be2a551198d0dc2f8b6f2145e511
  B=b777f68868ccf129c78904ffa0ffc20b4819023710e451a771543a2ff561a119
  C=d9ed24da16d30a631a78fd3a8de3062a6033ca579d1e3461f463870f338fe906
  D=274da400d4730d1bb1a9d2aae169757b32d8272c956849fc96686d3309a267e2


  $ start_and_wait_for_mononoke_server


  $ MASTER_CS_OUTPUT=$TESTTMP/master_cs_graph_output
  $ LATEST_CS_OUTPUT=$TESTTMP/latest_cs_graph_output
  $ OLDEST_COMMIT_TS_OUTPUT=$TESTTMP/oldest_commit_ts_graph_output
  $ LATEST_CS_AND_OLDEST_COMMIT_OUTPUT=$TESTTMP/latest_cs_and_oldest_commit_output

  $ B_AUTHOR_TS=1451613600


Specify a bookmark
  $ gitexport --log-level ERROR --repo-name "repo" -B "master" -p $EXPORT_DIR --partial-graph-output "$MASTER_CS_OUTPUT" --distance-limit 30

  $ cat $MASTER_CS_OUTPUT
  o  message: Add files to all directories, id: 565ba318c542ac2257113bdc31881e129939042ba10354e41605880c1f397cc8
  │   File changes:
  │  	 ADDED/MODIFIED: export_dir/C.txt 3e8ba6ef6107965afc1446b5b24533d9865204f1ea617672930d202f932bb892
  │  	 ADDED/MODIFIED: export_dir/subdir_to_export/second_subdir_export.txt e6d9f9d3bdd71e9c2dddec53da3bf447734da86b3897a7f7afd69cc7ac0cf3f1
  │
  o  message: Add subdirectory to export dir, id: ef0f4051b96d92f5d8e0521c4abbb63c9cd7f8d6324866d0be6b2dfe3b0c9973
  │   File changes:
  │  	 ADDED/MODIFIED: export_dir/subdir_to_export/export_file_in_subdir.txt e6d9f9d3bdd71e9c2dddec53da3bf447734da86b3897a7f7afd69cc7ac0cf3f1
  │
  o  message: Add files to export dir, id: 69f4d052996dc4a3fba7ab86939f567ad5a9be2a551198d0dc2f8b6f2145e511
      File changes:
     	 ADDED/MODIFIED: export_dir/B.txt 3e8ba6ef6107965afc1446b5b24533d9865204f1ea617672930d202f932bb892


Specify a changeset id
  $ gitexport --log-level ERROR --repo-name "repo" -p $EXPORT_DIR -i "$C" --partial-graph-output "$LATEST_CS_OUTPUT" --distance-limit 30

  $ cat $LATEST_CS_OUTPUT
  o  message: Add subdirectory to export dir, id: ef0f4051b96d92f5d8e0521c4abbb63c9cd7f8d6324866d0be6b2dfe3b0c9973
  │   File changes:
  │  	 ADDED/MODIFIED: export_dir/subdir_to_export/export_file_in_subdir.txt e6d9f9d3bdd71e9c2dddec53da3bf447734da86b3897a7f7afd69cc7ac0cf3f1
  │
  o  message: Add files to export dir, id: 69f4d052996dc4a3fba7ab86939f567ad5a9be2a551198d0dc2f8b6f2145e511
      File changes:
     	 ADDED/MODIFIED: export_dir/B.txt 3e8ba6ef6107965afc1446b5b24533d9865204f1ea617672930d202f932bb892

Test oldest commit timestamp arg
  $ gitexport --log-level ERROR --repo-name "repo" -B "master" -p $EXPORT_DIR --oldest-commit-ts $B_AUTHOR_TS --partial-graph-output "$OLDEST_COMMIT_TS_OUTPUT" --distance-limit 30

  $ cat $OLDEST_COMMIT_TS_OUTPUT
  o  message: Add files to all directories, id: 4978da5e38b8cd80c2f7c48fb66fa4006bab6a537a4ccef50d393ffaecb2804e
  │   File changes:
  │  	 ADDED/MODIFIED: export_dir/C.txt 3e8ba6ef6107965afc1446b5b24533d9865204f1ea617672930d202f932bb892
  │  	 ADDED/MODIFIED: export_dir/subdir_to_export/second_subdir_export.txt e6d9f9d3bdd71e9c2dddec53da3bf447734da86b3897a7f7afd69cc7ac0cf3f1
  │
  o  message: Add subdirectory to export dir, id: 37bb6d51a4fcc191fdf9b194c2968cbde2f572624f0c2163ce64cdc9999aa05e
      File changes:
     	 ADDED/MODIFIED: export_dir/subdir_to_export/export_file_in_subdir.txt e6d9f9d3bdd71e9c2dddec53da3bf447734da86b3897a7f7afd69cc7ac0cf3f1

Test both latest changeset and commit timestamp arg
  $ gitexport --log-level ERROR --repo-name "repo" -p $EXPORT_DIR -i "$C" --oldest-commit-ts $B_AUTHOR_TS --partial-graph-output "$LATEST_CS_AND_OLDEST_COMMIT_OUTPUT" --distance-limit 30

  $ cat $LATEST_CS_AND_OLDEST_COMMIT_OUTPUT
  o  message: Add subdirectory to export dir, id: 37bb6d51a4fcc191fdf9b194c2968cbde2f572624f0c2163ce64cdc9999aa05e
      File changes:
     	 ADDED/MODIFIED: export_dir/subdir_to_export/export_file_in_subdir.txt e6d9f9d3bdd71e9c2dddec53da3bf447734da86b3897a7f7afd69cc7ac0cf3f1
