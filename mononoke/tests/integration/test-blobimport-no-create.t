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
  $ drawdag <<EOF
  > C
  > |
  > B
  > |
  > A
  > EOF

create master bookmark
  $ hg bookmark master_bookmark -r tip

blobimport --no-create with no storage present, should fail due to missing directory
  $ cd ..
  $ blobimport --log repo-hg/.hg repo --no-create
  * using repo "repo" repoid RepositoryId(0)* (glob)
  E* Execution error: "$TESTTMP/blobstore/blobs" not found in ExistingOnly mode (glob)
  Error: Execution failed
  [1]

blobimport, succeeding, creates directory if not existing
  $ blobimport --log repo-hg/.hg repo
  * using repo "repo" repoid RepositoryId(0) (glob)
  * inserted commits # 0 (glob)
  * finished uploading changesets, globalrevs and deriving data (glob)
  * uploaded chunk of 1 bookmarks (glob)
  * latest imported revision 2 (glob)

check the bookmark is there after import
  $ mononoke_admin --readonly-storage bookmarks log master_bookmark 2>&1 | grep master_bookmark
  (master_bookmark) 26805aba1e600a82e93661149f2313866a221a7b blobimport * (glob)

blobimport --no-create after successful import, should be fine as storage shared with previous good run
  $ blobimport --log repo-hg/.hg repo --no-create
  * using repo "repo" repoid RepositoryId(0) (glob)
  * inserted commits # 0 (glob)
  * finished uploading changesets, globalrevs and deriving data (glob)
  * uploaded chunk of 0 bookmarks (glob)
  * latest imported revision 2 (glob)
