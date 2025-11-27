# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setconfig push.edenapi=true
  $ setup_common_config

  $ cd $TESTTMP

setup repo

  $ testtool_drawdag -R repo << EOF
  > A
  > # modify: A a "a file content"
  > # bookmark: A master_bookmark
  > EOF
  A=d672564be4c568b4d175fb2283de2485ea31cbe1d632ff2a6850b69e2940bad8

start mononoke
  $ start_and_wait_for_mononoke_server

setup push source repo
  $ hg clone -q mono:repo repo2


create new commit in repo2 and check that push fails

  $ cd repo2
  $ mkdir b_dir
  $ echo "new a file content" > a
  $ echo "b file content" > b_dir/b
  $ hg add b_dir/b
  $ hg ci -mb --extra "change-xrepo-mapping-to-version=somemapping"

  $ hg push -r . --to master_bookmark
  pushing rev 78905258cfae to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 2 files for upload
  edenapi: uploaded 2 files
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 1 changeset
  pushrebasing stack (b6a5e5b72f07, 78905258cfae] (1 commit) to remote bookmark master_bookmark
  abort: Server error: internal error: Disallowed extra change-xrepo-mapping-to-version is set on de811c645f9988b31e1ebbc3a740511c57f49011c6718242abc944ebbd50258e.
  
  Caused by:
      0: Disallowed extra change-xrepo-mapping-to-version is set on de811c645f9988b31e1ebbc3a740511c57f49011c6718242abc944ebbd50258e.
      1: Disallowed extra change-xrepo-mapping-to-version is set on de811c645f9988b31e1ebbc3a740511c57f49011c6718242abc944ebbd50258e.
  [255]


  $ killandwait $MONONOKE_PID
  $ cd "$TESTTMP"
  $ rm -rf "$TESTTMP/mononoke-config"
  $ ALLOW_CHANGE_XREPO_MAPPING_EXTRA=true setup_common_config
  $ mononoke
  $ wait_for_mononoke
  $ cd "$TESTTMP/repo2"
  $ hg push -r . --to master_bookmark
  pushing rev 78905258cfae to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  pushrebasing stack (b6a5e5b72f07, 78905258cfae] (1 commit) to remote bookmark master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to 78905258cfae
