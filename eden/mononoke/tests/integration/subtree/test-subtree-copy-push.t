# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Override subtree key to enable non-test subtree extra
  $ setconfig subtree.use-prod-subtree-key=True
  $ setconfig push.edenapi=true
  $ setconfig subtree.min-path-depth=1
  $ enable amend
  $ setup_common_config

  $ testtool_drawdag -R repo --derive-all --no-default-files << EOF
  > A-B-C
  > # modify: A foo/file1 "aaa\n"
  > # modify: A foo/file3 "xxx\n"
  > # copy: B foo/file2 "bbb\n" A foo/file1
  > # delete: B foo/file1
  > # modify: C foo/file2 "ccc\n"
  > # bookmark: C master_bookmark
  > EOF
  A=bad79679db57d8ca7bdcb80d082d1508f33ca2989652922e2e01b55fb3c27f6a
  B=170dbba760afb7ec239d859e2412a827dd7229cdbdfcd549b7138b2451afad37
  C=e611f471e1f2bd488fee752800983cdbfd38d50247e5d81222e0d620fd2a6120

  $ start_and_wait_for_mononoke_server
  $ hg clone -q mono:repo repo
  $ cd repo 

  $ hg subtree copy -r .^ --from-path foo --to-path bar
  copying foo to bar
  $ ls bar
  file2
  file3
  $ cat bar/file2
  bbb

  $ hg push -r . --to master_bookmark
  pushing rev d2dacebb0b05 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 1 changeset
  pushrebasing stack (3527857ec5dd, d2dacebb0b05] (1 commit) to remote bookmark master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to d2dacebb0b05

  $ hg subtree copy -r . --from-path foo --to-path baz
  copying foo to baz
  $ echo yyy >> baz/file3
  $ hg amend
  $ ls baz
  file2
  file3
  $ hg push -r . --to master_bookmark
  pushing rev a3c72378540f to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 2 files for upload
  edenapi: uploaded 2 files
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 1 changeset
  pushrebasing stack (d2dacebb0b05, a3c72378540f] (1 commit) to remote bookmark master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to a3c72378540f
  $ ls
  bar
  baz
  foo
