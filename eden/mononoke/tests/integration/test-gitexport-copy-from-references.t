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

# Test case to assert how `copy_from` values in FileChanges are handled
# depending if the source and destination paths are exported or not.
#
# This is important to track because in some scenarios export paths are created
# by copying files from a non-export path, which means that the history will
# not be entirely followed unless the user re-runs the tool passing the old
# path and the commit where the rename happened as its head commit.
#
#
# Commit E: source NOT exported and destination is exported.
#   **Expectation:** Print warning about creation of the export path and drop the copy_from reference
# Commit F: both source and destination are **NOT** exported.
#   **Expectation:** no warning is printed and the `copy_from` value is dropped.
# Commit H: both source and destination are exported.
#   **Expectation:** no warning is printed (because the export path is not being created) and `copy_from` field **is set and references the new parent**.
# Commit K: source is exported and destination is **NOT**.
#   **Expectation:** nothing happens because destination is not exported, so this file change will be ignored by the multi mover.


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
  > # copy: F "$INTERNAL_DIR/copied_internal.txt" "Internal file" E "$INTERNAL_DIR/internal.txt"
  > # modify: F "$EXPORT_DIR/A.txt" "Changing file"
  > # message: F "Modify internal and exported files"
  > # modify: G "$EXPORT_DIR/B.txt" "Changing file"
  > # message: G "Modify only exported file"
  > # copy: H "$EXPORT_DIR/second_subdir_export.txt" "Modify file to export" G "$EXPORT_DIR/B.txt"
  > # message: H "Modify only file in export directory"
  > # modify: I "$INTERNAL_DIR/another_internal.txt" "Changing file"
  > # message: I "Modify only file in internal root"
  > # delete: J "$EXPORT_DIR/second_subdir_export.txt"
  > # delete: J "$INTERNAL_DIR/another_internal.txt"
  > # message: J "Delete internal and exported files"
  > # copy: K "root_file.txt" "Copied from export file" J "$EXPORT_DIR/B.txt"
  > # message: K "Add file to repo root"
  > # bookmark: K master
  > EOF
  A=e954e5fb1ffc69119df10c1ed3218c1f28a32a1951d77367c868a57eb0ae8f53
  B=396a68afccbbf0d39c9be52eff16b3e87026de18468d15ee0e7dca9b33b97c2c
  C=4f918989900c17e32ee024fdcd634bb9beab540d7916c1941f737022baf41452
  D=659ed19d0148b13710d4d466e39a5d86d52e6dabfe3becd8dbfb7e02fe327abc
  E=6fc3f51f797aecf2a419fb70362d7da614bf5a7c1fc7ca067af0bdccff817493
  F=6f2bbddd552711fd6a7eab98b1e9b0ca8a6fbb3fb5c39de68b788fa79458e152
  G=4c1edb7a7f9f86a7098ecb06615aea40838c5a3cef98a261ba67079e03267571
  H=f8d343e720829a8b9dee101fb1ee43d2b55a70f55569bacefc53ef9ebf40b864
  I=85c0ad6e387568a6b637860336ae167bbd33c2ed5a6f04cb6e871f1a08032b0b
  J=4dd08e820f8c8ad0ba3acc01018ba53d98e98b99da1d40f5767f3185657212c5
  K=5281096a3beb73fb6530c3fe4f25e7ae184822df90bc91942b33987103bf192f

  $ start_and_wait_for_mononoke_server


# -------------------- Use the gitexport tool --------------------


  $ SOURCE_GRAPH_OUTPUT=$TESTTMP/source_graph_output
  $ PARTIAL_GRAPH_OUTPUT=$TESTTMP/partial_graph_output

Run the tool without passing the old name as an export path

  $ gitexport --log-level WARN --repo-name "repo" -B "master" -p "$EXPORT_DIR" --source-graph-output "$SOURCE_GRAPH_OUTPUT" --partial-graph-output "$PARTIAL_GRAPH_OUTPUT" --distance-limit 30
  *] Changeset ChangesetId(Blake2(6fc3f51f797aecf2a419fb70362d7da614bf5a7c1fc7ca067af0bdccff817493)) might have created one of the exported paths by moving/copying files from a previous commit that will not be exported (id ChangesetId(Blake2(659ed19d0148b13710d4d466e39a5d86d52e6dabfe3becd8dbfb7e02fe327abc))). (glob)
  *] Changeset ChangesetId(Blake2(6f2bbddd552711fd6a7eab98b1e9b0ca8a6fbb3fb5c39de68b788fa79458e152)) might have created one of the exported paths by moving/copying files from a previous commit that will not be exported (id ChangesetId(Blake2(6fc3f51f797aecf2a419fb70362d7da614bf5a7c1fc7ca067af0bdccff817493))). (glob)

  $ diff --old-line-format="- %L" --new-line-format="+ %L" "$SOURCE_GRAPH_OUTPUT" "$PARTIAL_GRAPH_OUTPUT"
  - o  message: Add file to repo root
  - │   File changes:
  - │  	 COPY/MOVE: root_file.txt ac6ac47201405136170fea99eff9e0e589a14e51b92253d2105327af3ce51892
  - │
  o  message: Delete internal and exported files
  │   File changes:
  │  	 REMOVED: export_dir/second_subdir_export.txt
  - │  	 REMOVED: internal_dir/another_internal.txt
  - │
  - o  message: Modify only file in internal root
  - │   File changes:
  - │  	 ADDED/MODIFIED: internal_dir/another_internal.txt a6ef1a0dddad73cbfd4ce3bd9642f5aab0c4ae1fcb58af3cacda2f0ed914efd8
  │
  o  message: Modify only file in export directory
  │   File changes:
  │  	 COPY/MOVE: export_dir/second_subdir_export.txt 641106875cd2090a0019d25d920cf9015eb4036f1ece30b8fbb7dd5be785f9c4
  │
  o  message: Modify only exported file
  │   File changes:
  │  	 ADDED/MODIFIED: export_dir/B.txt a6ef1a0dddad73cbfd4ce3bd9642f5aab0c4ae1fcb58af3cacda2f0ed914efd8
  │
  o  message: Modify internal and exported files
  │   File changes:
  │  	 ADDED/MODIFIED: export_dir/A.txt a6ef1a0dddad73cbfd4ce3bd9642f5aab0c4ae1fcb58af3cacda2f0ed914efd8
  - │  	 COPY/MOVE: internal_dir/copied_internal.txt dbc317c4f0146e8a455e9bc8eea646248145c962b3f4689c22285d3c8b25fd5e
  │
  o  message: Rename export directory
  - │   File changes:
  - │  	 COPY/MOVE: export_dir/B.txt 3e8ba6ef6107965afc1446b5b24533d9865204f1ea617672930d202f932bb892
  - │  	 COPY/MOVE: export_dir/C.txt 641106875cd2090a0019d25d920cf9015eb4036f1ece30b8fbb7dd5be785f9c4
  - │  	 REMOVED: old_export_dir/B.txt
  - │  	 REMOVED: old_export_dir/C.txt
  - │
  - o  message: Add file to internal_dir
  - │   File changes:
  - │  	 ADDED/MODIFIED: internal_dir/internal.txt dbc317c4f0146e8a455e9bc8eea646248145c962b3f4689c22285d3c8b25fd5e
  - │
  - o  message: Modify files in both directories
  - │   File changes:
  - │  	 ADDED/MODIFIED: internal_dir/another_internal.txt dbc317c4f0146e8a455e9bc8eea646248145c962b3f4689c22285d3c8b25fd5e
  - │  	 ADDED/MODIFIED: old_export_dir/C.txt 641106875cd2090a0019d25d920cf9015eb4036f1ece30b8fbb7dd5be785f9c4
  - │
  - o  message: Add another export file
  - │   File changes:
  - │  	 ADDED/MODIFIED: old_export_dir/C.txt bc10fa4c7856280755c757a75dafadb36d7e5f105cdfeedbcdbc76dab37a708a
  - │
  - o  message: Add files to export dir before rename
      File changes:
  -    	 ADDED/MODIFIED: old_export_dir/B.txt 3e8ba6ef6107965afc1446b5b24533d9865204f1ea617672930d202f932bb892
  +    	 ADDED/MODIFIED: export_dir/B.txt 3e8ba6ef6107965afc1446b5b24533d9865204f1ea617672930d202f932bb892
  +    	 ADDED/MODIFIED: export_dir/C.txt 641106875cd2090a0019d25d920cf9015eb4036f1ece30b8fbb7dd5be785f9c4
  [1]


Run the tool and pass the old name manually as an export path using the bounded export paths file arg

  $ rm $SOURCE_GRAPH_OUTPUT $PARTIAL_GRAPH_OUTPUT
  $ BOUNDED_EXPORT_PATHS_FILE=$TESTTMP/bounded_export_paths.json
  $ echo '[ { "path": '"\"$OLD_EXPORT_DIR\""', "head": { "ID": '"\"$E\""' }  } ]' > $BOUNDED_EXPORT_PATHS_FILE

  $ gitexport --log-level ERROR --repo-name "repo" -B "master" -p $EXPORT_DIR --source-graph-output "$SOURCE_GRAPH_OUTPUT" --partial-graph-output "$PARTIAL_GRAPH_OUTPUT" --distance-limit 30 -f "$BOUNDED_EXPORT_PATHS_FILE"


  $ diff --old-line-format="- %L" --new-line-format="+ %L" "$SOURCE_GRAPH_OUTPUT" "$PARTIAL_GRAPH_OUTPUT"
  - o  message: Add file to repo root
  - │   File changes:
  - │  	 COPY/MOVE: root_file.txt ac6ac47201405136170fea99eff9e0e589a14e51b92253d2105327af3ce51892
  - │
  o  message: Delete internal and exported files
  │   File changes:
  │  	 REMOVED: export_dir/second_subdir_export.txt
  - │  	 REMOVED: internal_dir/another_internal.txt
  - │
  - o  message: Modify only file in internal root
  - │   File changes:
  - │  	 ADDED/MODIFIED: internal_dir/another_internal.txt a6ef1a0dddad73cbfd4ce3bd9642f5aab0c4ae1fcb58af3cacda2f0ed914efd8
  │
  o  message: Modify only file in export directory
  │   File changes:
  │  	 COPY/MOVE: export_dir/second_subdir_export.txt 641106875cd2090a0019d25d920cf9015eb4036f1ece30b8fbb7dd5be785f9c4
  │
  o  message: Modify only exported file
  │   File changes:
  │  	 ADDED/MODIFIED: export_dir/B.txt a6ef1a0dddad73cbfd4ce3bd9642f5aab0c4ae1fcb58af3cacda2f0ed914efd8
  │
  o  message: Modify internal and exported files
  │   File changes:
  │  	 ADDED/MODIFIED: export_dir/A.txt a6ef1a0dddad73cbfd4ce3bd9642f5aab0c4ae1fcb58af3cacda2f0ed914efd8
  - │  	 COPY/MOVE: internal_dir/copied_internal.txt dbc317c4f0146e8a455e9bc8eea646248145c962b3f4689c22285d3c8b25fd5e
  │
  o  message: Rename export directory
  │   File changes:
  │  	 COPY/MOVE: export_dir/B.txt 3e8ba6ef6107965afc1446b5b24533d9865204f1ea617672930d202f932bb892
  │  	 COPY/MOVE: export_dir/C.txt 641106875cd2090a0019d25d920cf9015eb4036f1ece30b8fbb7dd5be785f9c4
  │  	 REMOVED: old_export_dir/B.txt
  │  	 REMOVED: old_export_dir/C.txt
  │
  - o  message: Add file to internal_dir
  - │   File changes:
  - │  	 ADDED/MODIFIED: internal_dir/internal.txt dbc317c4f0146e8a455e9bc8eea646248145c962b3f4689c22285d3c8b25fd5e
  - │
  o  message: Modify files in both directories
  │   File changes:
  - │  	 ADDED/MODIFIED: internal_dir/another_internal.txt dbc317c4f0146e8a455e9bc8eea646248145c962b3f4689c22285d3c8b25fd5e
  │  	 ADDED/MODIFIED: old_export_dir/C.txt 641106875cd2090a0019d25d920cf9015eb4036f1ece30b8fbb7dd5be785f9c4
  │
  o  message: Add another export file
  │   File changes:
  │  	 ADDED/MODIFIED: old_export_dir/C.txt bc10fa4c7856280755c757a75dafadb36d7e5f105cdfeedbcdbc76dab37a708a
  │
  o  message: Add files to export dir before rename
      File changes:
     	 ADDED/MODIFIED: old_export_dir/B.txt 3e8ba6ef6107965afc1446b5b24533d9865204f1ea617672930d202f932bb892
  [1]
