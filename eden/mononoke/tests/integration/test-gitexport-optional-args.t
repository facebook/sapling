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
  > # bookmark: D master
  > EOF
  A=2b45b0cac2615a6b5f1808161f96eb56376f313b45744ce83fd60931dee1e02b
  B=db859048f5ffc6d47dddd3bbe01e223654e9992537421e4ba13b87a7e0dbcc3c
  C=18ecf80ae5c1d7f1ca4d86f0679553c96be5aff1fb7b6dfa7b6343c0cde461a5
  D=b1075aab50713f6440222a3e8729d874fab9e3276fd97057ebda2bea4fc27e68

  $ start_and_wait_for_mononoke_server


  $ MASTER_CS_OUTPUT=$TESTTMP/master_cs_graph_output
  $ LATEST_CS_OUTPUT=$TESTTMP/latest_cs_graph_output


Specify a bookmark
  $ gitexport --log-level ERROR --repo-name "repo" -B "master" -p $EXPORT_DIR --partial-graph-output "$MASTER_CS_OUTPUT" --distance-limit 30

  $ cat $MASTER_CS_OUTPUT
  o  message: Add files to all directories, id: 994707996002ba7d453fce0668b883d056f5a168318d94f905d55891ccf0a331
  │   File changes:
  │  	 ADDED/MODIFIED: export_dir/C.txt 3e8ba6ef6107965afc1446b5b24533d9865204f1ea617672930d202f932bb892
  │  	 ADDED/MODIFIED: export_dir/subdir_to_export/second_subdir_export.txt e6d9f9d3bdd71e9c2dddec53da3bf447734da86b3897a7f7afd69cc7ac0cf3f1
  │
  o  message: Add subdirectory to export dir, id: 736ffbf2afbe183ef8caaa7e148ca208cad27de8b0436443a836393654b3a658
  │   File changes:
  │  	 ADDED/MODIFIED: export_dir/subdir_to_export/export_file_in_subdir.txt e6d9f9d3bdd71e9c2dddec53da3bf447734da86b3897a7f7afd69cc7ac0cf3f1
  │
  o  message: Add files to export dir, id: 2b45b0cac2615a6b5f1808161f96eb56376f313b45744ce83fd60931dee1e02b
      File changes:
     	 ADDED/MODIFIED: export_dir/B.txt 3e8ba6ef6107965afc1446b5b24533d9865204f1ea617672930d202f932bb892


Specify a changeset id
  $ gitexport --log-level ERROR --repo-name "repo" -p $EXPORT_DIR -i "$C" --partial-graph-output "$LATEST_CS_OUTPUT" --distance-limit 30

  $ cat $LATEST_CS_OUTPUT
  o  message: Add subdirectory to export dir, id: 736ffbf2afbe183ef8caaa7e148ca208cad27de8b0436443a836393654b3a658
  │   File changes:
  │  	 ADDED/MODIFIED: export_dir/subdir_to_export/export_file_in_subdir.txt e6d9f9d3bdd71e9c2dddec53da3bf447734da86b3897a7f7afd69cc7ac0cf3f1
  │
  o  message: Add files to export dir, id: 2b45b0cac2615a6b5f1808161f96eb56376f313b45744ce83fd60931dee1e02b
      File changes:
     	 ADDED/MODIFIED: export_dir/B.txt 3e8ba6ef6107965afc1446b5b24533d9865204f1ea617672930d202f932bb892
