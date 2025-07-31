# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

use test subtree key so server does not follow subtree copy for blame
  $ setconfig subtree.use-prod-subtree-key=False
  $ setconfig push.edenapi=true
  $ setconfig subtree.min-path-depth=1
  $ setconfig experimental.edenapi-blame=True
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
  pushing rev 6b0de5ec81d4 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 1 changeset
  pushrebasing stack (3527857ec5dd, 6b0de5ec81d4] (1 commit) to remote bookmark master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to 6b0de5ec81d4

  $ hg log -G -T '{node|short} {desc}\n'
  @  6b0de5ec81d4 Subtree copy from 5e5fb79d0ae8540eb249997e17f5e593cb78ee1b
  │  - Copied path foo to bar
  o  3527857ec5dd C
  │
  o  5e5fb79d0ae8 B
  │
  o  7ac2e4266f1b A
  
  $ hg blame bar/file3
  7ac2e4266f1b: xxx

  $ hg subtree copy -r . --from-path foo --to-path baz
  copying foo to baz
  $ echo yyy >> baz/file3
  $ hg amend
  $ ls baz
  file2
  file3
  $ hg push -r . --to master_bookmark
  pushing rev 35ee8b2028dd to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 2 files for upload
  edenapi: uploaded 2 files
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 1 changeset
  pushrebasing stack (6b0de5ec81d4, 35ee8b2028dd] (1 commit) to remote bookmark master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to 35ee8b2028dd
  $ ls
  bar
  baz
  foo


  $ hg log -G -T '{node|short} {desc}\n'
  @  35ee8b2028dd Subtree copy from 6b0de5ec81d4708de2d1ca96daa70a80e765d4de
  │  - Copied path foo to baz
  o  6b0de5ec81d4 Subtree copy from 5e5fb79d0ae8540eb249997e17f5e593cb78ee1b
  │  - Copied path foo to bar
  o  3527857ec5dd C
  │
  o  5e5fb79d0ae8 B
  │
  o  7ac2e4266f1b A
  
  $ hg blame baz/file3
  7ac2e4266f1b: xxx
  35ee8b2028dd: yyy

  $ echo zzz >> baz/file3
  $ hg ci -m "add zzz"
  $ hg push -r . --to master_bookmark -q
  $ hg blame baz/file3
  7ac2e4266f1b: xxx
  35ee8b2028dd: yyy
  984613ca7101: zzz
