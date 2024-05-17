# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

Setup configuration
  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/gitexport_library.sh"
  $ cd $TESTTMP



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


# -------------------- Use the gitexport tool --------------------


Run the tool without passing the old name as an export path

  $ test_gitexport --log-level="ERROR" -p "foo/a" -p "bar"


  $ git clone "$GIT_BUNDLE_OUTPUT" "$GIT_REPO"
  Cloning into '$TESTTMP/git_repo'...

  $ diff_hg_and_git_repos  
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
