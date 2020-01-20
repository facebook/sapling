# Copyright (c) Facebook, Inc. and its affiliates.
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

check the read sql path still works with --readonly-storage
  $ mononoke_admin --readonly-storage bookmarks log master_bookmark 2>&1 | grep master_bookmark
  (master_bookmark) 26805aba1e600a82e93661149f2313866a221a7b blobimport * (glob)

check that sql writes are blocked by --readonly-storage
  $ mononoke_admin --readonly-storage bookmarks set another_bookmark 26805aba1e600a82e93661149f2313866a221a7b 2>&1
  * using repo "repo" repoid * (glob)
  * changeset resolved as: * (glob)
  * While executing ReplaceBookmarks query (glob)
  
  Caused by:
      attempt to write a readonly database
  [1]
