# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

# setup repo, usefncache flag for forcing algo encoding run
  $ hginit_treemanifest repo-hg --config format.usefncache=False
  $ cd repo-hg
  $ echo hello > world
  $ hg commit -Aqm "some commit"
  $ hg bookmark -r . master

  $ REPOID=0 setup_common_config
  $ cd $TESTTMP
  $ REPOID=0 blobimport repo-hg/.hg repo
  $ mononoke_admin bookmarks --repo-id=0 list
  * master (glob)
  $ rm -rf repo

  $ REPOID=1 setup_common_config
  $ cd $TESTTMP
  $ REPOID=1 blobimport repo-hg/.hg repo --no-bookmark
  $ mononoke_admin bookmarks --repo-id=1 list
  $ rm -rf repo

  $ REPOID=2 setup_common_config
  $ cd $TESTTMP
  $ REPOID=2 blobimport repo-hg/.hg repo --prefix-bookmark myrepo/
  $ mononoke_admin bookmarks --repo-id=2 list
  * myrepo/master (glob)
