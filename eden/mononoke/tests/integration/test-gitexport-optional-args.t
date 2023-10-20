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
  $ hgmn_clone mononoke://$(mononoke_address)/repo repo
  $ cd repo
  $ hg -q co master
  $ SOURCE_REPO_LOG=$TESTTMP/source_repo_log
  $ hg log --git --template "{firstline(desc)}\n{stat()}\n" | sed -E 's/\s+\|\s+([0-9]+).+/ \| \1/' > $SOURCE_REPO_LOG

  $ B_AUTHOR_TS=1451613600

  $ GIT_BUNDLE_OUTPUT=git_bundle
  $ GIT_REPO=git_repo
  $ GIT_REPO_LOG=$TESTTMP/git_repo_log


Specify a bookmark
  $ gitexport --log-level ERROR --repo-name "repo" -B "master" -p $EXPORT_DIR -o "$GIT_BUNDLE_OUTPUT"
  $ git clone $GIT_BUNDLE_OUTPUT $GIT_REPO
  Cloning into 'git_repo'...
  $ cd $GIT_REPO

  $ git log --stat --pretty=format:"%s" | sed -E 's/\s+\|\s+([0-9]+).+/ \| \1/' > $GIT_REPO_LOG

  $ diff --old-line-format="- %L" --new-line-format="+ %L" "$SOURCE_REPO_LOG" "$GIT_REPO_LOG"
  Add files to all directories
   export_dir/C.txt | 1
   export_dir/subdir_to_export/second_subdir_export.txt | 1
  -  internal_dir/another_internal.txt | 1
  -  3 files changed, 3 insertions(+), 0 deletions(-)
  +  2 files changed, 2 insertions(+)
  
  Add subdirectory to export dir
   export_dir/subdir_to_export/export_file_in_subdir.txt | 1
  -  1 files changed, 1 insertions(+), 0 deletions(-)
  - 
  - Add file to internal_dir
  -  internal_dir/internal.txt | 1
  -  1 files changed, 1 insertions(+), 0 deletions(-)
  +  1 file changed, 1 insertion(+)
  
  Add files to export dir
   export_dir/B.txt | 1
  -  1 files changed, 1 insertions(+), 0 deletions(-)
  - 
  +  1 file changed, 1 insertion(+)
  [1]




  $ rm -rf $GIT_BUNDLE_OUTPUT $GIT_REPO $GIT_REPO_LOG


Specify a changeset id
  $ gitexport --log-level ERROR --repo-name "repo" -p $EXPORT_DIR -i "$C" -o "$GIT_BUNDLE_OUTPUT"
  $ git clone $GIT_BUNDLE_OUTPUT $GIT_REPO
  Cloning into 'git_repo'...
  $ cd $GIT_REPO

  $ git log --stat --pretty=format:"%s" | sed -E 's/\s+\|\s+([0-9]+).+/ \| \1/' > $GIT_REPO_LOG

  $ diff --old-line-format="- %L" --new-line-format="+ %L" "$SOURCE_REPO_LOG" "$GIT_REPO_LOG"
  - Add files to all directories
  -  export_dir/C.txt | 1
  -  export_dir/subdir_to_export/second_subdir_export.txt | 1
  -  internal_dir/another_internal.txt | 1
  -  3 files changed, 3 insertions(+), 0 deletions(-)
  - 
  Add subdirectory to export dir
   export_dir/subdir_to_export/export_file_in_subdir.txt | 1
  -  1 files changed, 1 insertions(+), 0 deletions(-)
  - 
  - Add file to internal_dir
  -  internal_dir/internal.txt | 1
  -  1 files changed, 1 insertions(+), 0 deletions(-)
  +  1 file changed, 1 insertion(+)
  
  Add files to export dir
   export_dir/B.txt | 1
  -  1 files changed, 1 insertions(+), 0 deletions(-)
  - 
  +  1 file changed, 1 insertion(+)
  [1]


  $ rm -rf $GIT_BUNDLE_OUTPUT $GIT_REPO $GIT_REPO_LOG


Test oldest commit timestamp arg
  $ gitexport --log-level ERROR --repo-name "repo" -B "master" -p $EXPORT_DIR --oldest-commit-ts $B_AUTHOR_TS -o "$GIT_BUNDLE_OUTPUT"
  $ git clone $GIT_BUNDLE_OUTPUT $GIT_REPO
  Cloning into 'git_repo'...
  $ cd $GIT_REPO

  $ git log --stat --pretty=format:"%s" | sed -E 's/\s+\|\s+([0-9]+).+/ \| \1/' > $GIT_REPO_LOG

  $ diff --old-line-format="- %L" --new-line-format="+ %L" "$SOURCE_REPO_LOG" "$GIT_REPO_LOG"
  Add files to all directories
   export_dir/C.txt | 1
   export_dir/subdir_to_export/second_subdir_export.txt | 1
  -  internal_dir/another_internal.txt | 1
  -  3 files changed, 3 insertions(+), 0 deletions(-)
  +  2 files changed, 2 insertions(+)
  
  Add subdirectory to export dir
   export_dir/subdir_to_export/export_file_in_subdir.txt | 1
  -  1 files changed, 1 insertions(+), 0 deletions(-)
  - 
  - Add file to internal_dir
  -  internal_dir/internal.txt | 1
  -  1 files changed, 1 insertions(+), 0 deletions(-)
  - 
  - Add files to export dir
  -  export_dir/B.txt | 1
  -  1 files changed, 1 insertions(+), 0 deletions(-)
  - 
  +  1 file changed, 1 insertion(+)
  [1]


  $ rm -rf $GIT_BUNDLE_OUTPUT $GIT_REPO $GIT_REPO_LOG



Test both latest changeset and commit timestamp arg
  $ gitexport --log-level ERROR --repo-name "repo" -p $EXPORT_DIR -i "$C" --oldest-commit-ts $B_AUTHOR_TS -o "$GIT_BUNDLE_OUTPUT"
  $ git clone $GIT_BUNDLE_OUTPUT $GIT_REPO
  Cloning into 'git_repo'...
  $ cd $GIT_REPO

  $ git log --stat --pretty=format:"%s" | sed -E 's/\s+\|\s+([0-9]+).+/ \| \1/' > $GIT_REPO_LOG

  $ diff --old-line-format="- %L" --new-line-format="+ %L" "$SOURCE_REPO_LOG" "$GIT_REPO_LOG"
  - Add files to all directories
  -  export_dir/C.txt | 1
  -  export_dir/subdir_to_export/second_subdir_export.txt | 1
  -  internal_dir/another_internal.txt | 1
  -  3 files changed, 3 insertions(+), 0 deletions(-)
  - 
  Add subdirectory to export dir
   export_dir/subdir_to_export/export_file_in_subdir.txt | 1
  -  1 files changed, 1 insertions(+), 0 deletions(-)
  - 
  - Add file to internal_dir
  -  internal_dir/internal.txt | 1
  -  1 files changed, 1 insertions(+), 0 deletions(-)
  - 
  - Add files to export dir
  -  export_dir/B.txt | 1
  -  1 files changed, 1 insertions(+), 0 deletions(-)
  - 
  +  1 file changed, 1 insertion(+)
  [1]
