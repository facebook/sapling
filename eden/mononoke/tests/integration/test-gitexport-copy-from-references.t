# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

Setting up a simple scenario for the gitexport tool
  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/gitexport_library.sh"
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
  $ hgmn_clone mononoke://$(mononoke_address)/repo repo
  $ cd repo
  $ hg -q co master

# -------------------- Use the gitexport tool --------------------


Run the tool without passing the old name as an export path

  $ test_gitexport --log-level WARN -p "$EXPORT_DIR"
  *] Changeset 6fc3f51f797aecf2a419fb70362d7da614bf5a7c1fc7ca067af0bdccff817493 might have created the exported path export_dir by moving/copying files from a commit that might not be exported (id 659ed19d0148b13710d4d466e39a5d86d52e6dabfe3becd8dbfb7e02fe327abc). (glob)

  $ git clone $GIT_BUNDLE_OUTPUT $GIT_REPO
  Cloning into '$TESTTMP/git_repo'...
 
  $ diff_hg_and_git_repos
  - Add file to repo root
  -  root_file.txt | 1
  -  1 files changed, 1 insertions(+), 0 deletions(-)
  - 
  Delete internal and exported files
   export_dir/second_subdir_export.txt | 1
  -  internal_dir/another_internal.txt | 1
  -  2 files changed, 0 insertions(+), 2 deletions(-)
  - 
  - Modify only file in internal root
  -  internal_dir/another_internal.txt | 2
  -  1 files changed, 1 insertions(+), 1 deletions(-)
  +  1 file changed, 1 deletion(-)
  
  Modify only file in export directory
   export_dir/second_subdir_export.txt | 1
  -  1 files changed, 1 insertions(+), 0 deletions(-)
  +  1 file changed, 1 insertion(+)
  
  Modify only exported file
   export_dir/B.txt | 2
  -  1 files changed, 1 insertions(+), 1 deletions(-)
  +  1 file changed, 1 insertion(+), 1 deletion(-)
  
  Modify internal and exported files
   export_dir/A.txt | 1
  -  internal_dir/copied_internal.txt | 1
  -  2 files changed, 2 insertions(+), 0 deletions(-)
  +  1 file changed, 1 insertion(+)
  
  Rename export directory
   export_dir/B.txt | 1
   export_dir/C.txt | 1
  -  old_export_dir/B.txt | 1
  -  old_export_dir/C.txt | 1
  -  4 files changed, 2 insertions(+), 2 deletions(-)
  - 
  - Add file to internal_dir
  -  internal_dir/internal.txt | 1
  -  1 files changed, 1 insertions(+), 0 deletions(-)
  - 
  - Modify files in both directories
  -  internal_dir/another_internal.txt | 1
  -  old_export_dir/C.txt | 2
  -  2 files changed, 2 insertions(+), 1 deletions(-)
  - 
  - Add another export file
  -  old_export_dir/C.txt | 1
  -  1 files changed, 1 insertions(+), 0 deletions(-)
  - 
  - Add files to export dir before rename
  -  old_export_dir/B.txt | 1
  -  1 files changed, 1 insertions(+), 0 deletions(-)
  - 
  +  2 files changed, 2 insertions(+)


Run the tool and pass the old name manually as an export path using the bounded export paths file arg

  $ BOUNDED_EXPORT_PATHS_FILE=$TESTTMP/bounded_export_paths.json
  $ echo '[ { "paths": '["\"$OLD_EXPORT_DIR\""]', "head": { "ID": '"\"$E\""' }  } ]' > $BOUNDED_EXPORT_PATHS_FILE

  $ test_gitexport --log-level ERROR -p $EXPORT_DIR -f "$BOUNDED_EXPORT_PATHS_FILE"

  $ git clone $GIT_BUNDLE_OUTPUT $GIT_REPO
  Cloning into '$TESTTMP/git_repo'...
  $ diff_hg_and_git_repos  
  - Add file to repo root
  -  root_file.txt | 1
  -  1 files changed, 1 insertions(+), 0 deletions(-)
  - 
  Delete internal and exported files
   export_dir/second_subdir_export.txt | 1
  -  internal_dir/another_internal.txt | 1
  -  2 files changed, 0 insertions(+), 2 deletions(-)
  - 
  - Modify only file in internal root
  -  internal_dir/another_internal.txt | 2
  -  1 files changed, 1 insertions(+), 1 deletions(-)
  +  1 file changed, 1 deletion(-)
  
  Modify only file in export directory
   export_dir/second_subdir_export.txt | 1
  -  1 files changed, 1 insertions(+), 0 deletions(-)
  +  1 file changed, 1 insertion(+)
  
  Modify only exported file
   export_dir/B.txt | 2
  -  1 files changed, 1 insertions(+), 1 deletions(-)
  +  1 file changed, 1 insertion(+), 1 deletion(-)
  
  Modify internal and exported files
   export_dir/A.txt | 1
  -  internal_dir/copied_internal.txt | 1
  -  2 files changed, 2 insertions(+), 0 deletions(-)
  +  1 file changed, 1 insertion(+)
  
  Rename export directory
  -  export_dir/B.txt | 1
  -  export_dir/C.txt | 1
  -  old_export_dir/B.txt | 1
  -  old_export_dir/C.txt | 1
  -  4 files changed, 2 insertions(+), 2 deletions(-)
  - 
  - Add file to internal_dir
  -  internal_dir/internal.txt | 1
  -  1 files changed, 1 insertions(+), 0 deletions(-)
  +  {old_export_dir => export_dir}/B.txt | 0
  +  {old_export_dir => export_dir}/C.txt | 0
  +  2 files changed, 0 insertions(+), 0 deletions(-)
  
  Modify files in both directories
  -  internal_dir/another_internal.txt | 1
   old_export_dir/C.txt | 2
  -  2 files changed, 2 insertions(+), 1 deletions(-)
  +  1 file changed, 1 insertion(+), 1 deletion(-)
  
  Add another export file
   old_export_dir/C.txt | 1
  -  1 files changed, 1 insertions(+), 0 deletions(-)
  +  1 file changed, 1 insertion(+)
  
  Add files to export dir before rename
   old_export_dir/B.txt | 1
  -  1 files changed, 1 insertions(+), 0 deletions(-)
  - 
  +  1 file changed, 1 insertion(+)
