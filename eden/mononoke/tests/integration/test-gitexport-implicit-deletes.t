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
  $ hgmn_clone mononoke://$(mononoke_address)/repo repo
  $ cd repo
  $ hg -q co master
  $ SOURCE_REPO_LOG=$TESTTMP/source_repo_log
  $ hg log --git --template "{firstline(desc)}\n{stat()}\n" | sed -E 's/\s+\|\s+([0-9]+).+/ \| \1/' > $SOURCE_REPO_LOG



# -------------------- Use the gitexport tool --------------------


  $ GIT_REPO_OUTPUT="$TESTTMP/git_bundle"
  $ GIT_REPO_LOG=$TESTTMP/git_repo_log


Run the tool without passing the old name as an export path

  $ gitexport --log-level ERROR --repo-name "repo" -B "master" -p "foo/a" -p "bar" -o "$GIT_REPO_OUTPUT"


  $ git clone "$GIT_REPO_OUTPUT" git_repo
  Cloning into 'git_repo'...
  $ cd git_repo

  $ git log --stat --pretty=format:"%s" | sed -E 's/\s+\|\s+([0-9]+).+/ \| \1/' > $GIT_REPO_LOG

  $ diff --old-line-format="- %L" --new-line-format="+ %L" "$SOURCE_REPO_LOG" "$GIT_REPO_LOG"
  C
   bar | 1
   bar/f/g | 1
   bar/h/i | 1
  -  3 files changed, 1 insertions(+), 2 deletions(-)
  +  3 files changed, 1 insertion(+), 2 deletions(-)
  
  B
  -  foo | 1
   foo/a/c | 1
   foo/a/d | 1
  -  foo/b/e | 1
  -  4 files changed, 1 insertions(+), 3 deletions(-)
  +  2 files changed, 2 deletions(-)
  
  A
   bar/f/g | 1
   bar/h/i | 1
   foo/a/c | 1
   foo/a/d | 1
  -  foo/b/e | 1
  -  5 files changed, 5 insertions(+), 0 deletions(-)
  - 
  +  4 files changed, 4 insertions(+)
  [1]
