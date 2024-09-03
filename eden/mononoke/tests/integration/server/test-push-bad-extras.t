# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setconfig push.edenapi=true
  $ ENABLE_API_WRITES=1 setup_common_config

  $ cd $TESTTMP

setup repo

  $ hginit_treemanifest repo
  $ cd repo
  $ echo "a file content" > a
  $ hg add a
  $ hg ci -ma

setup master bookmarks

  $ hg bookmark master_bookmark -r 'tip'

verify content
  $ hg log
  commit:      0e7ec5675652
  bookmark:    master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
   (re)

  $ cd $TESTTMP
  $ blobimport repo/.hg repo

setup push source repo
  $ hg clone -q mono:repo repo2

start mononoke

  $ start_and_wait_for_mononoke_server
create new commit in repo2 and check that push fails

  $ cd repo2
  $ mkdir b_dir
  $ echo "new a file content" > a
  $ echo "b file content" > b_dir/b
  $ hg add b_dir/b
  $ hg ci -mb --extra "change-xrepo-mapping-to-version=somemapping"

  $ hg push -r . --to master_bookmark
  pushing rev 9c40727be57c to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 2 files for upload
  edenapi: uploaded 2 files
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 1 changeset
  pushrebasing stack (0e7ec5675652, 9c40727be57c] (1 commit) to remote bookmark master_bookmark
  abort: Server error: internal error: Disallowed extra change-xrepo-mapping-to-version is set on 603d7b6d937e2a896edcfcf17dcf76bb8dfc644510db19b359bfb056d6299c5e.
  
  Caused by:
      0: Disallowed extra change-xrepo-mapping-to-version is set on 603d7b6d937e2a896edcfcf17dcf76bb8dfc644510db19b359bfb056d6299c5e.
      1: Disallowed extra change-xrepo-mapping-to-version is set on 603d7b6d937e2a896edcfcf17dcf76bb8dfc644510db19b359bfb056d6299c5e.
  [255]

  $ killandwait $MONONOKE_PID
  $ cd "$TESTTMP"
  $ rm -rf "$TESTTMP/mononoke-config"
  $ ENABLE_API_WRITES=1 ALLOW_CHANGE_XREPO_MAPPING_EXTRA=true setup_common_config
  $ mononoke
  $ wait_for_mononoke
  $ cd "$TESTTMP/repo2"
  $ hg push -r . --to master_bookmark
  pushing rev 9c40727be57c to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  pushrebasing stack (0e7ec5675652, 9c40727be57c] (1 commit) to remote bookmark master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to 9c40727be57c
