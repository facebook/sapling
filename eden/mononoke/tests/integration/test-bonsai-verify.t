# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ cd $TESTTMP

setup common configuration
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > EOF


setup repo
  $ testtool_drawdag -R repo << EOF
  > A
  > # modify: A a "a"
  > # author_date: A "1970-01-01T01:00:00+00:00"
  > # author: A test
  > EOF
  A=546ab8adb92af7ef882231ea89d5f3d6d1d0345f761aa7b3a25ff08f25aa0e85

  $ HG_ID=$(mononoke_newadmin convert --repo-name repo  --derive --from bonsai --to hg $A)


smoke test to ensure bonsai_verify works

  $ bonsai_verify round-trip $HG_ID 2>&1 | grep valid
  * 100.00% valid, summary: , total: 1, valid: 1, errors: 0, ignored: 0 (glob)
