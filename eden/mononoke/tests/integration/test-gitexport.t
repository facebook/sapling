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
  $ cd $TESTTMP


Set some env vars that will be used frequently

  $ EXPORT_DIR="export_dir"
-- Folder that should NOT be exported to the git repo
  $ INTERNAL_DIR="internal_dir"
-- Subdirectory in EXPORT_DIR that should not be exported
  $ FB_SUBDIR="$EXPORT_DIR/facebook"
  $ SECOND_EXPORT_DIR="second_export_dir"


# -------------------------- Create commits --------------------------
  $ testtool_drawdag -R repo --derive-all <<EOF
  > A-B-C-D-E-F-G-H-I
  > # modify: A "$EXPORT_DIR/B.txt" "File to export"
  > # message: A "Add files to export dir"
  > # modify: B "$INTERNAL_DIR/internal.txt" "Internal file"
  > # message: B "Add file to internal_dir"
  > # modify: C "$FB_DIR/exception_from_export.txt" "Internal file in exception folder"
  > # message: C "Add exception directory to export dir"
  > # modify: D "$FB_DIR/2nd_exception_from_export_dir.txt" "Internal file in exception folder"
  > # modify: D "$EXPORT_DIR/export_dir/C.txt" "File to export"
  > # modify: D "$INTERNAL_DIR/another_internal.txt" "Internal file"
  > # message: D "Add files to all directories"
  > # modify: E "$SECOND_EXPORT_DIR/another_file.txt" "Another file to export"
  > # message: E "Create another export directory"
  > # modify: F "$INTERNAL_DIR/internal.txt" "Changing file"
  > # modify: F "$EXPORT_DIR/A.txt" "Changing file"
  > # modify: F "$FB_DIR/exception_from_export_dir.txt" "Changing file"
  > # message: F "Modify internal and exported files"
  > # modify: G "$EXPORT_DIR/B.txt" "Changing file"
  > # message: G "Modify only exported file"
  > # modify: H "$FB_DIR/2nd_exception_from_export_dir.txt" "Changing file"
  > # message: H "Modify only file in exception folder"
  > # modify: I "$INTERNAL_DIR/another_internal.txt" "Changing file"
  > # message: I "Modify only file in internal root"
  > # bookmark: I main
  > EOF
  A=8b4acc9caa1cacc715912d4ea9c314db2d7028dd10f64553eb99620be92bb830
  B=8e8f391769a0bddcd8af193d0721531eb4766b5cd4fc22ffda0f5233fa7dbe19
  C=051d574d6d5e12428ef853f7880d0652677f8cf59bebf80c9227f009f36105d6
  D=0f23d869bfbfa7d096524f0df979bc338a68c0ff2394c792209ed0015c68210a
  E=49d0241f406a42212d94b4ca40dea808f757132ced9512fed044936fc503fff9
  F=eaaccaee56ade23100dea1550ab076eadc447b772f62adb6bd871e9c6505148d
  G=7bd6f9b27f56175b08eb32a2ab6663b4dff203665cf782df6d83a99d132e1edb
  H=82d17e92c6fb604c17efbc3daa6afab60728f3a576b185eaa41a1bf2aa512d0f
  I=a8f6bcc345058aca2f28dac45d6b54b89e923bda7c75423bebf7678eaf3dd8eb

  $ start_and_wait_for_mononoke_server
  $ hgmn_clone mononoke://$(mononoke_address)/repo repo
  $ cd repo
  $ hg -q co main


# Finish creating commits
# ------------------------------------------------------------------------------


Check all the commits
  $ hg sl
  @  commit:      7e52b1ef6f84
  │  bookmark:    main
  │  user:        author
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     Modify only file in internal root
  │
  o  commit:      1f6147d06261
  │  user:        author
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     Modify only file in exception folder
  │
  o  commit:      a435687d6bae
  │  user:        author
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     Modify only exported file
  │
  o  commit:      fa672b512392
  │  user:        author
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     Modify internal and exported files
  │
  o  commit:      cf9999253960
  │  user:        author
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     Create another export directory
  │
  o  commit:      40ce49af8415
  │  user:        author
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     Add files to all directories
  │
  o  commit:      88517b405cd8
  │  user:        author
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     Add exception directory to export dir
  │
  o  commit:      e839bab2b2da
  │  user:        author
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     Add file to internal_dir
  │
  o  commit:      55f1431a4e73
     user:        author
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     Add files to export dir
  




# -------------------- Use the gitexport tool --------------------

Set location of binary, resources and options (e.g. output path, directories)
# TODO(T160600991): Pass the CLI pass once the initial binary is setup in buck
  $ GITEXPORT_CLI=""

# Path that should be exported to the git repo
  $ EXPORT_PATHS="$EXPORT_DIR $SECOND_EXPORT_DIR"

  $ HG_REPO="$TESTTMP/repo"

  $ GIT_REPO_OUTPUT="$TESTTMP/git_repo"

# TODO(T160600443): support optional first/last commits
# NOTE: these would take precedence over the start/end date arguments.
  $ FIRST_COMMIT=""

  $ LAST_COMMIT=""

# TODO(T160600443): support optional start/end date arguments
  $ START_DATE="2023-01-01"

  $ END_DATE="2023-02-01"

Run the tool

# TODO(T160600991): uncomment once the CLI binary is created
# $ $GITEXPORT_CLI --hg-repo "$REPO" --output "$GIT_REPO_OUTPUT" --export-paths "$EXPORT_PATHS"



# -------------------- Run checks on the git repo --------------------


# $ cd "$GIT_REPO_OUTPUT"


# TODO(T160600934): count number of commits
# TODO(T160600934): assert paths are correct
# TODO(T160600934): confirm no internal files are there
