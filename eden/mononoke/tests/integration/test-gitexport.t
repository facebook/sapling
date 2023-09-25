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
  $ SECOND_EXPORT_DIR="second_export_dir"


# -------------------------- Create commits --------------------------
  $ testtool_drawdag -R repo --derive-all --no-default-files <<EOF
  > A-B-C-D-E-F-G-H-I-J-K
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
  > # modify: E "$SECOND_EXPORT_DIR/another_file.txt" "Another file to export"
  > # message: E "Create another export directory"
  > # modify: F "$INTERNAL_DIR/internal.txt" "Changing file"
  > # modify: F "$EXPORT_DIR/A.txt" "Changing file"
  > # modify: F "$EXPORT_SUBDIR/exception_from_export_dir.txt" "Changing file"
  > # message: F "Modify internal and exported files"
  > # modify: G "$EXPORT_DIR/B.txt" "Changing file"
  > # message: G "Modify only exported file"
  > # modify: H "$EXPORT_SUBDIR/second_subdir_export.txt" "Changing file"
  > # message: H "Modify only file in export subdirectory"
  > # modify: I "$INTERNAL_DIR/another_internal.txt" "Changing file"
  > # message: I "Modify only file in internal root"
  > # delete: J "$EXPORT_SUBDIR/second_subdir_export.txt"
  > # delete: J "$INTERNAL_DIR/another_internal.txt"
  > # message: J "Delete internal and exported files"
  > # modify: K "root_file.txt" "Root file"
  > # message: K "Add file to repo root"
  > # bookmark: K master
  > EOF
  A=2b45b0cac2615a6b5f1808161f96eb56376f313b45744ce83fd60931dee1e02b
  B=db859048f5ffc6d47dddd3bbe01e223654e9992537421e4ba13b87a7e0dbcc3c
  C=18ecf80ae5c1d7f1ca4d86f0679553c96be5aff1fb7b6dfa7b6343c0cde461a5
  D=b1075aab50713f6440222a3e8729d874fab9e3276fd97057ebda2bea4fc27e68
  E=bf427657abaa0a5b88cf50295ba5c5639f45b89cc67e15f7bc5c2b496c84bff9
  F=22bf902c5e155b92caddfe384693a69f379cdada5277ab524a8dbfddc5ab2077
  G=ae2469ceeba5ee03e6501c85b7335c1fa5fa8e75a5de678743037d6e8c220c47
  H=aad9a55aa109275b392b829d09c571caa4add25753c6a6d547d753534e8ddc89
  I=83f4af124d0b2052d090ca254150f6fa4d5dc9303ffd23c601d1f7a6dc23892e
  J=56abf334447e5deb10163335caf2477aa105a8bee096627de06222f01d45c65d
  K=ca1b7e33632b3b9a89abe7f820b590f1185cf7e187386e9bddf4c1cbe62dc324

  $ start_and_wait_for_mononoke_server

# Finish creating commits

# -------------------- Use the gitexport tool --------------------

Set location of binary, resources and options (e.g. output path, directories)
# Path that should be exported to the git repo
  $ EXPORT_PATHS=($EXPORT_DIR $SECOND_EXPORT_DIR)

  $ SOURCE_GRAPH_OUTPUT=$TESTTMP/source_graph_output
  $ PARTIAL_GRAPH_OUTPUT=$TESTTMP/partial_graph_output

  $ GIT_REPO_OUTPUT="$TESTTMP/git_repo"

# TODO(T160600443): support optional start/end date arguments
  $ START_DATE="2023-01-01"

  $ END_DATE="2023-02-01"

Run the tool

  $ gitexport --log-level ERROR --repo-name "repo" -B "master" $(printf -- '-p %s ' "${EXPORT_PATHS[@]}") --source-graph-output "$SOURCE_GRAPH_OUTPUT" --partial-graph-output "$PARTIAL_GRAPH_OUTPUT" --distance-limit 30

  $ diff --old-line-format="- %L" --new-line-format="+ %L" "$SOURCE_GRAPH_OUTPUT" "$PARTIAL_GRAPH_OUTPUT"
  - o  message: Add file to repo root, id: ca1b7e33632b3b9a89abe7f820b590f1185cf7e187386e9bddf4c1cbe62dc324
  - │   File changes:
  - │  	 ADDED/MODIFIED: root_file.txt 1fc392f47d2822cab18c09dd980ea6bff4c0af4f55249fd01696b5ae04b8f30f
  - │
  - o  message: Delete internal and exported files, id: 56abf334447e5deb10163335caf2477aa105a8bee096627de06222f01d45c65d
  + o  message: Delete internal and exported files, id: 125c12a5605e721287dda77a62975461902083347436356664fa6e38be01e714
  │   File changes:
  │  	 REMOVED: export_dir/subdir_to_export/second_subdir_export.txt
  - │  	 REMOVED: internal_dir/another_internal.txt
  - │
  - o  message: Modify only file in internal root, id: 83f4af124d0b2052d090ca254150f6fa4d5dc9303ffd23c601d1f7a6dc23892e
  - │   File changes:
  - │  	 ADDED/MODIFIED: internal_dir/another_internal.txt a6ef1a0dddad73cbfd4ce3bd9642f5aab0c4ae1fcb58af3cacda2f0ed914efd8
  │
  - o  message: Modify only file in export subdirectory, id: aad9a55aa109275b392b829d09c571caa4add25753c6a6d547d753534e8ddc89
  + o  message: Modify only file in export subdirectory, id: 56afc1f97612fee7e67df5ca86cdd0705cc74a079ebdc15e08c139d77a588597
  │   File changes:
  │  	 ADDED/MODIFIED: export_dir/subdir_to_export/second_subdir_export.txt a6ef1a0dddad73cbfd4ce3bd9642f5aab0c4ae1fcb58af3cacda2f0ed914efd8
  │
  - o  message: Modify only exported file, id: ae2469ceeba5ee03e6501c85b7335c1fa5fa8e75a5de678743037d6e8c220c47
  + o  message: Modify only exported file, id: 2a1902253c3076731ae693822c4399974975c13f1be892925026a7df0e143d01
  │   File changes:
  │  	 ADDED/MODIFIED: export_dir/B.txt a6ef1a0dddad73cbfd4ce3bd9642f5aab0c4ae1fcb58af3cacda2f0ed914efd8
  │
  - o  message: Modify internal and exported files, id: 22bf902c5e155b92caddfe384693a69f379cdada5277ab524a8dbfddc5ab2077
  + o  message: Modify internal and exported files, id: 1f7a5e225e6207d0d8604173a13f3592173e07efa8804ffc20381d5595179009
  │   File changes:
  │  	 ADDED/MODIFIED: export_dir/A.txt a6ef1a0dddad73cbfd4ce3bd9642f5aab0c4ae1fcb58af3cacda2f0ed914efd8
  │  	 ADDED/MODIFIED: export_dir/subdir_to_export/exception_from_export_dir.txt a6ef1a0dddad73cbfd4ce3bd9642f5aab0c4ae1fcb58af3cacda2f0ed914efd8
  - │  	 ADDED/MODIFIED: internal_dir/internal.txt a6ef1a0dddad73cbfd4ce3bd9642f5aab0c4ae1fcb58af3cacda2f0ed914efd8
  │
  - o  message: Create another export directory, id: bf427657abaa0a5b88cf50295ba5c5639f45b89cc67e15f7bc5c2b496c84bff9
  + o  message: Create another export directory, id: e018441040174d40efefea1717dd81112159ab716ca1fdfb19e7ddf4d5ae4f0f
  │   File changes:
  │  	 ADDED/MODIFIED: second_export_dir/another_file.txt 5edfe2d7d203cc580278e40f794da385c8895e8cad8803e176d305a5bf48e406
  │
  - o  message: Add files to all directories, id: b1075aab50713f6440222a3e8729d874fab9e3276fd97057ebda2bea4fc27e68
  + o  message: Add files to all directories, id: 994707996002ba7d453fce0668b883d056f5a168318d94f905d55891ccf0a331
  │   File changes:
  │  	 ADDED/MODIFIED: export_dir/C.txt 3e8ba6ef6107965afc1446b5b24533d9865204f1ea617672930d202f932bb892
  │  	 ADDED/MODIFIED: export_dir/subdir_to_export/second_subdir_export.txt e6d9f9d3bdd71e9c2dddec53da3bf447734da86b3897a7f7afd69cc7ac0cf3f1
  - │  	 ADDED/MODIFIED: internal_dir/another_internal.txt dbc317c4f0146e8a455e9bc8eea646248145c962b3f4689c22285d3c8b25fd5e
  │
  - o  message: Add subdirectory to export dir, id: 18ecf80ae5c1d7f1ca4d86f0679553c96be5aff1fb7b6dfa7b6343c0cde461a5
  + o  message: Add subdirectory to export dir, id: 736ffbf2afbe183ef8caaa7e148ca208cad27de8b0436443a836393654b3a658
  │   File changes:
  │  	 ADDED/MODIFIED: export_dir/subdir_to_export/export_file_in_subdir.txt e6d9f9d3bdd71e9c2dddec53da3bf447734da86b3897a7f7afd69cc7ac0cf3f1
  - │
  - o  message: Add file to internal_dir, id: db859048f5ffc6d47dddd3bbe01e223654e9992537421e4ba13b87a7e0dbcc3c
  - │   File changes:
  - │  	 ADDED/MODIFIED: internal_dir/internal.txt dbc317c4f0146e8a455e9bc8eea646248145c962b3f4689c22285d3c8b25fd5e
  │
  o  message: Add files to export dir, id: 2b45b0cac2615a6b5f1808161f96eb56376f313b45744ce83fd60931dee1e02b
      File changes:
     	 ADDED/MODIFIED: export_dir/B.txt 3e8ba6ef6107965afc1446b5b24533d9865204f1ea617672930d202f932bb892
  [1]

# -------------------- Run checks on the git repo --------------------


# $ cd "$GIT_REPO_OUTPUT"


# TODO(T160600934): count number of commits
# TODO(T160600934): assert paths are correct
# TODO(T160600934): confirm no internal files are there
