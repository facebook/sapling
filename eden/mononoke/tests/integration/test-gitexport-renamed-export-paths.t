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

  $ OLD_EXPORT_DIR="old_export_dir"
  $ EXPORT_DIR="export_dir"
  $ INTERNAL_DIR="internal_dir" # Folder that should NOT be exported to the git repo

# Test case to cover scenarios where an exported directory was created by
# renaming another.
# In this case, we want to follow the history and export the changesets affecting
# the directory with the old name.

# NOTE: Creating a history where there's an irrelevant commit (commit D)
# between one that modifies files in the old export path name (commit C) and
# the one that renames the export directory (commit E).
  $ testtool_drawdag -R repo --derive-all --no-default-files <<EOF
  > A-B-C-D-E-F-G-H-I-J-K
  > # modify: A "$OLD_EXPORT_DIR/B.txt" "File to export"
  > # message: A "Add files to export dir before rename"
  > # modify: B "$OLD_EXPORT_DIR/C.txt" "Another export file"
  > # message: B "Add another export file"
  > # modify: C "$OLD_EXPORT_DIR/C.txt" "Modify file to export"
  > # modify: C "$INTERNAL_DIR/another_internal.txt" "Internal file"
  > # message: C "Modify files in both directories"
  > # modify: D "$INTERNAL_DIR/internal.txt" "Internal file"
  > # message: D "Add file to internal_dir"
  > # copy: E "$EXPORT_DIR/B.txt" "File to export" D "$OLD_EXPORT_DIR/B.txt"
  > # copy: E "$EXPORT_DIR/C.txt" "Modify file to export" D "$OLD_EXPORT_DIR/C.txt"
  > # delete: E "$OLD_EXPORT_DIR/B.txt"
  > # delete: E "$OLD_EXPORT_DIR/C.txt"
  > # message: E "Rename export directory"
  > # modify: F "$INTERNAL_DIR/internal.txt" "Changing file"
  > # modify: F "$EXPORT_DIR/A.txt" "Changing file"
  > # message: F "Modify internal and exported files"
  > # modify: G "$EXPORT_DIR/B.txt" "Changing file"
  > # message: G "Modify only exported file"
  > # modify: H "$EXPORT_DIR/second_subdir_export.txt" "Changing file"
  > # message: H "Modify only file in export directory"
  > # modify: I "$INTERNAL_DIR/another_internal.txt" "Changing file"
  > # message: I "Modify only file in internal root"
  > # delete: J "$EXPORT_DIR/second_subdir_export.txt"
  > # delete: J "$INTERNAL_DIR/another_internal.txt"
  > # message: J "Delete internal and exported files"
  > # modify: K "root_file.txt" "Root file"
  > # message: K "Add file to repo root"
  > # bookmark: K master
  > EOF
  A=e954e5fb1ffc69119df10c1ed3218c1f28a32a1951d77367c868a57eb0ae8f53
  B=396a68afccbbf0d39c9be52eff16b3e87026de18468d15ee0e7dca9b33b97c2c
  C=4f918989900c17e32ee024fdcd634bb9beab540d7916c1941f737022baf41452
  D=659ed19d0148b13710d4d466e39a5d86d52e6dabfe3becd8dbfb7e02fe327abc
  E=6fc3f51f797aecf2a419fb70362d7da614bf5a7c1fc7ca067af0bdccff817493
  F=824be851b343d7d43e08d55b59a4bb57dadf7db4639044f79804764af286999a
  G=7f0bc8f6714d877194f074b9f8436bd3798cc183170d8707fb465e815807467b
  H=6b215d19cbf41a739e60176eac37c84bc50c118f5f4eb99bff5102f30a2ee617
  I=31de873264a0d07db554437559f01bd0827b84d051e8daa15c7f97d06693ff4a
  J=aeabdc90a1716382f1c7ebb4bb956339bb5cc12e0df11e8419266a37979839f2
  K=7616c9e240de5b549f4c1e5331d45419a783191c76a79bc6711c3eabd5148802

  $ start_and_wait_for_mononoke_server


# -------------------- Use the gitexport tool --------------------


  $ SOURCE_GRAPH_OUTPUT=$TESTTMP/source_graph_output
  $ PARTIAL_GRAPH_OUTPUT=$TESTTMP/partial_graph_output
  $ EXPORT_PATHS=($EXPORT_DIR $OLD_EXPORT_DIR)


Run the tool and pass the old name manually as an export path

  $ gitexport --log-level ERROR --repo-name "repo" -B "master" $(printf -- '-p %s ' "${EXPORT_PATHS[@]}") --source-graph-output "$SOURCE_GRAPH_OUTPUT" --partial-graph-output "$PARTIAL_GRAPH_OUTPUT" --distance-limit 30 2>&1 | sed -E "s|.+(Execution error.+)|\1|g"

  $ diff --old-line-format="- %L" --new-line-format="+ %L" "$SOURCE_GRAPH_OUTPUT" "$PARTIAL_GRAPH_OUTPUT"
  - o  message: Add file to repo root, id: 7616c9e240de5b549f4c1e5331d45419a783191c76a79bc6711c3eabd5148802
  - │   File changes:
  - │  	 ADDED/MODIFIED: root_file.txt 1fc392f47d2822cab18c09dd980ea6bff4c0af4f55249fd01696b5ae04b8f30f
  - │
  - o  message: Delete internal and exported files, id: aeabdc90a1716382f1c7ebb4bb956339bb5cc12e0df11e8419266a37979839f2
  + o  message: Delete internal and exported files, id: 73e0fdf2b165f0cba39609aeac5c26e26ccd93a1ebf716b6f16bb32158302638
  │   File changes:
  │  	 REMOVED: export_dir/second_subdir_export.txt
  - │  	 REMOVED: internal_dir/another_internal.txt
  - │
  - o  message: Modify only file in internal root, id: 31de873264a0d07db554437559f01bd0827b84d051e8daa15c7f97d06693ff4a
  - │   File changes:
  - │  	 ADDED/MODIFIED: internal_dir/another_internal.txt a6ef1a0dddad73cbfd4ce3bd9642f5aab0c4ae1fcb58af3cacda2f0ed914efd8
  │
  - o  message: Modify only file in export directory, id: 6b215d19cbf41a739e60176eac37c84bc50c118f5f4eb99bff5102f30a2ee617
  + o  message: Modify only file in export directory, id: 3da1557cac829bac10107042602f4b02b2a49bbb81a29935a1b2e0e8de6ea11f
  │   File changes:
  │  	 ADDED/MODIFIED: export_dir/second_subdir_export.txt a6ef1a0dddad73cbfd4ce3bd9642f5aab0c4ae1fcb58af3cacda2f0ed914efd8
  │
  - o  message: Modify only exported file, id: 7f0bc8f6714d877194f074b9f8436bd3798cc183170d8707fb465e815807467b
  + o  message: Modify only exported file, id: ae0280134c93155c6c507b3dbfa8042e1c7e7b496aa2fa8ded3edbeb53a05db0
  │   File changes:
  │  	 ADDED/MODIFIED: export_dir/B.txt a6ef1a0dddad73cbfd4ce3bd9642f5aab0c4ae1fcb58af3cacda2f0ed914efd8
  │
  - o  message: Modify internal and exported files, id: 824be851b343d7d43e08d55b59a4bb57dadf7db4639044f79804764af286999a
  + o  message: Modify internal and exported files, id: 3a09baac9785409a5dc34d8e9d819fdb96c6e77804af6959828ac6dc488acd1b
  │   File changes:
  │  	 ADDED/MODIFIED: export_dir/A.txt a6ef1a0dddad73cbfd4ce3bd9642f5aab0c4ae1fcb58af3cacda2f0ed914efd8
  - │  	 ADDED/MODIFIED: internal_dir/internal.txt a6ef1a0dddad73cbfd4ce3bd9642f5aab0c4ae1fcb58af3cacda2f0ed914efd8
  │
  - o  message: Rename export directory, id: 6fc3f51f797aecf2a419fb70362d7da614bf5a7c1fc7ca067af0bdccff817493
  + o  message: Rename export directory, id: 240037cf23cca5b2371dc7694a815a31c8b6c65edf294451b08f9c14e51df2a7
  │   File changes:
  │  	 COPY/MOVE: export_dir/B.txt 3e8ba6ef6107965afc1446b5b24533d9865204f1ea617672930d202f932bb892
  │  	 COPY/MOVE: export_dir/C.txt 641106875cd2090a0019d25d920cf9015eb4036f1ece30b8fbb7dd5be785f9c4
  │  	 REMOVED: old_export_dir/B.txt
  │  	 REMOVED: old_export_dir/C.txt
  │
  - o  message: Add file to internal_dir, id: 659ed19d0148b13710d4d466e39a5d86d52e6dabfe3becd8dbfb7e02fe327abc
  - │   File changes:
  - │  	 ADDED/MODIFIED: internal_dir/internal.txt dbc317c4f0146e8a455e9bc8eea646248145c962b3f4689c22285d3c8b25fd5e
  - │
  - o  message: Modify files in both directories, id: 4f918989900c17e32ee024fdcd634bb9beab540d7916c1941f737022baf41452
  + o  message: Modify files in both directories, id: dacc5bfab5ebe3eb88aca57d282b0aa9dfb02b492fce79e5de7f209776d77a0f
  │   File changes:
  - │  	 ADDED/MODIFIED: internal_dir/another_internal.txt dbc317c4f0146e8a455e9bc8eea646248145c962b3f4689c22285d3c8b25fd5e
  │  	 ADDED/MODIFIED: old_export_dir/C.txt 641106875cd2090a0019d25d920cf9015eb4036f1ece30b8fbb7dd5be785f9c4
  │
  o  message: Add another export file, id: 396a68afccbbf0d39c9be52eff16b3e87026de18468d15ee0e7dca9b33b97c2c
  │   File changes:
  │  	 ADDED/MODIFIED: old_export_dir/C.txt bc10fa4c7856280755c757a75dafadb36d7e5f105cdfeedbcdbc76dab37a708a
  │
  o  message: Add files to export dir before rename, id: e954e5fb1ffc69119df10c1ed3218c1f28a32a1951d77367c868a57eb0ae8f53
      File changes:
     	 ADDED/MODIFIED: old_export_dir/B.txt 3e8ba6ef6107965afc1446b5b24533d9865204f1ea617672930d202f932bb892
  [1]

Run the tool without passing the old name as an export path

  $ rm $SOURCE_GRAPH_OUTPUT $PARTIAL_GRAPH_OUTPUT

  $ gitexport --log-level ERROR --repo-name "repo" -B "master" -p $EXPORT_DIR --source-graph-output "$SOURCE_GRAPH_OUTPUT" --partial-graph-output "$PARTIAL_GRAPH_OUTPUT" --distance-limit 30 2>&1 | sed -E "s|.+(Execution error.+)|\1|g"
  Execution error: internal error: Remapped commit 659ed19d0148b13710d4d466e39a5d86d52e6dabfe3becd8dbfb7e02fe327abc expected in target repo, but not present
  
  Caused by:
      0: Remapped commit 659ed19d0148b13710d4d466e39a5d86d52e6dabfe3becd8dbfb7e02fe327abc expected in target repo, but not present
      1: Remapped commit 659ed19d0148b13710d4d466e39a5d86d52e6dabfe3becd8dbfb7e02fe327abc expected in target repo, but not present
  Error: Execution failed


  $ diff --old-line-format="- %L" --new-line-format="+ %L" "$SOURCE_GRAPH_OUTPUT" "$PARTIAL_GRAPH_OUTPUT"
  diff: $TESTTMP/partial_graph_output: No such file or directory
  [2]
