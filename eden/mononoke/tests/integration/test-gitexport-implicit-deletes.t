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


# Test if implicit deletes are being handled properly the following scenarios:
# a) Directory implicitly deletes is a parent of exported directory but it's not
# exported itself (`foo`).
#   Expected: there will be DELETION file changes for all files under the
#   implicitly deleted directory.
#
# b) Directory implicitly deletes is exported (`bar`)
#   Expected: no DELETION file changes are needed, because the new file is
#   exported and will express the deletion of the files implicitly in the new
#   new bonsai.
  $ testtool_drawdag -R repo --derive-all --no-default-files <<EOF
  > A-B-C
  > # modify: A "foo/a/c" "c"
  > # modify: A "foo/a/d" "d"
  > # modify: A "foo/b/e" "e"
  > # modify: A "bar/f/g" "g"
  > # modify: A "bar/h/i" "i"
  > # modify: B "foo" "SECRET FILE"
  > # modify: C "bar" "SECRET FILE"
  > # bookmark: C master
  > EOF
  A=85dfabda124636200fe6499b65123179020d32c0ab50818b72a8097dcf9b1880
  B=a0cf4ce8cd4495b5733f844cc384dbcce4c305eb597a73f38d239cad78c29883
  C=5bfc3bc38a36db879cf0c7215f91df159c4b6cf9ebb9fc6e33faf17ed00a1860

  $ start_and_wait_for_mononoke_server


# -------------------- Use the gitexport tool --------------------


  $ SOURCE_GRAPH_OUTPUT=$TESTTMP/source_graph_output
  $ PARTIAL_GRAPH_OUTPUT=$TESTTMP/partial_graph_output



Run the tool without passing the old name as an export path

  $ gitexport --log-level ERROR --repo-name "repo" -B "master" -p "foo/a" -p "bar" --source-graph-output "$SOURCE_GRAPH_OUTPUT" --partial-graph-output "$PARTIAL_GRAPH_OUTPUT"

  $ diff --old-line-format="- %L" --new-line-format="+ %L" "$SOURCE_GRAPH_OUTPUT" "$PARTIAL_GRAPH_OUTPUT"
  o  message: C
  │   File changes:
  │  	 ADDED/MODIFIED: bar 2d94831a5f092c12e63b2ad909ca07a673aa166792d2a2024a786911c9e56f80
  │
  o  message: B
  │   File changes:
  - │  	 ADDED/MODIFIED: foo 2d94831a5f092c12e63b2ad909ca07a673aa166792d2a2024a786911c9e56f80
  + │  	 REMOVED: foo/a/c
  + │  	 REMOVED: foo/a/d
  │
  o  message: A
      File changes:
     	 ADDED/MODIFIED: bar/f/g 6691ce7f2f393958665fc2c0ceba62d392676ba6599674d4e2d581336a366aa1
     	 ADDED/MODIFIED: bar/h/i ab427c4f5720c8cc1ad19b7fee41e18ad535e3da92cdbfd7259d21ec856b50f2
     	 ADDED/MODIFIED: foo/a/c 000a1a9b74aa3da71fcceb653a62cb6987ae440c2b5c3d7e5d08d7c526b1dca8
     	 ADDED/MODIFIED: foo/a/d 05c656843a2628dcc13b383c1028c59dc9adb362ffc7cff0506f6e4afc850fe8
  -    	 ADDED/MODIFIED: foo/b/e 1aeca865722255fcab5e8906eca61bf338ff57cc31b10ee097255ea43fd267e1
  [1]
