# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

Setup configuration
  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/gitexport_library.sh"
  $ cd $TESTTMP


# In this scenario there's a `copy_from` reference that doesn't create
# the export directory so no warning should be printed to avoid
# wasting user's time inspecting the commits.
  $ testtool_drawdag -R repo --derive-all --no-default-files <<EOF
  > A-B-C
  > # modify: A "foo/a.txt" "creating foo directory"
  > # modify: A "bar/b.txt" "creating bar directory"
  > # modify: B "bar/c.txt" "irrelevant change"
  > # copy: C "foo/b.txt" "copying file from bar into foo" B "bar/b.txt"
  > # bookmark: C master
  > EOF
  A=7e97054c51a17ea2c03cd5184826b6a7556d141d57c5a1641bbd62c0854d1a36
  B=00b258af738c278985fa2f7b224bb2054527eaaedfcc49d5e5cb0af35080d2f3
  C=5737239030ee7036172ee7bf8e2986159258fa401b4edb4c06d2c62cdb1e33c1


  $ start_and_wait_for_mononoke_server
  $ hgmn_clone mononoke://$(mononoke_address)/repo repo
  $ cd repo
  $ hg -q co master


# -------------------- Use the gitexport tool --------------------

# No warning should be printed because `foo` was created in `A`, not in `C`
# which contains the `copy_from` reference.
  $ test_gitexport --log-level WARN -p "foo"


  $ git clone $GIT_BUNDLE_OUTPUT $GIT_REPO
  Cloning into '$TESTTMP/git_repo'...

  $ diff_hg_and_git_repos
  C
   foo/b.txt | 1
  -  1 files changed, 1 insertions(+), 0 deletions(-)
  - 
  - B
  -  bar/c.txt | 1
  -  1 files changed, 1 insertions(+), 0 deletions(-)
  +  1 file changed, 1 insertion(+)
  
  A
  -  bar/b.txt | 1
   foo/a.txt | 1
  -  2 files changed, 2 insertions(+), 0 deletions(-)
  - 
  +  1 file changed, 1 insertion(+)
