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
  $ hg init repo-hg
  $ cd repo-hg
  $ setup_hg_server
  $ hg debugdrawdag <<EOF
  > C
  > |
  > B
  > |
  > A
  > EOF

create master bookmark
  $ hg bookmark master_bookmark -r tip

blobimport, succeeding
  $ cd ..
  $ rm -rf ./repo
  $ blobimport repo-hg/.hg repo

check the read sql path still works with readonly storage
  $ mononoke_newadmin --with-readonly-storage=true bookmarks -R repo log master_bookmark
  * (master_bookmark) c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd blobimport * (glob)

check that sql writes are blocked by readonly storage
  $ mononoke_newadmin --with-readonly-storage=true bookmarks -R repo set another_bookmark 26805aba1e600a82e93661149f2313866a221a7b
  Creating publishing bookmark another_bookmark at c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd
  * While executing InsertBookmarksImpl query (glob)
  
  Caused by:
      0: attempt to write a readonly database
      1: Error code 8: Attempt to write a readonly database
  [1]
