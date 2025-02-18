# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Override subtree key to enable non-test subtree extra
  $ cat > $TESTTMP/subtree.py <<EOF
  > from sapling.utils import subtreeutil
  > def extsetup(ui):
  >     subtreeutil.SUBTREE_KEY = "subtree"
  > EOF
  $ setconfig extensions.subtreetestoverride=$TESTTMP/subtree.py
  $ setconfig push.edenapi=true
  $ setconfig subtree.copy-reuse-tree=true subtree.min-path-depth=1
  $ enable amend
  $ setup_common_config

  $ testtool_drawdag -R repo --derive-all --no-default-files << EOF
  > A-B-C
  > # modify: A foo/file1 "aaa\n"
  > # copy: B foo/file2 "bbb\n" A foo/file1
  > # delete: B foo/file1
  > # modify: C foo/file2 "ccc\n"
  > # bookmark: C master_bookmark
  > EOF
  A=942068675aae3ea79427f460688d1776ab3e8e1696ea7373b0378f57d5de7700
  B=df2a0eaaf041a902fd13e2bb769356b05ff422199f65c076a2c905beb06c5e4f
  C=076c2409fdb896e34b7e70dbf43ad20861772bdcbb7f94fdd3f8a5b00c4fa2ec

  $ start_and_wait_for_mononoke_server
  $ hg clone -q mono:repo repo
  $ cd repo 

Subtree copies can be rebased, and they retain their original source location
(i.e., later changes to the source directory are ignored).

  $ hg update -q .^
  $ hg subtree copy -r .^ --from-path foo --to-path bar
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls bar
  file1
  $ cat bar/file1
  aaa
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  subtree=[{"copies":[{"from_commit":"13445855d10c80bc6ef92e531c44430ea1101b6e","from_path":"foo","to_path":"bar"}],"v":1}]

  $ hg log -G -T '{node|short} {desc|firstline} {remotebookmarks}\n'
  @  a925cd481025 Subtree copy from 13445855d10c80bc6ef92e531c44430ea1101b6e
  │
  │ o  d55124608f34 C remote/master_bookmark
  ├─╯
  o  8aeb486cc22e B
  │
  o  13445855d10c A
  
  $ hg push -r . --to master_bookmark
  pushing rev a925cd481025 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (8aeb486cc22e, a925cd481025] (1 commit) to remote bookmark master_bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to 416d3b39a0c6

  $ cat bar/file1
  aaa

Subtree copies should conflict with other subtree copies when pushrebasing.

  $ hg update -q .^
  $ hg subtree copy -r . --from-path foo --to-path bar
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls bar
  file2
  $ hg push -r . --to master_bookmark
  pushing rev 5ed56273856d to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 1 changeset
  pushrebasing stack (d55124608f34, 5ed56273856d] (1 commit) to remote bookmark master_bookmark
  abort: Server error: Conflicts while pushrebasing: [PushrebaseConflict { left: MPath("bar"), right: MPath("bar") }]
  [255]

Subtree copies should conflict with changes made to the destination (even if they are other files)

  $ hg update -q master_bookmark
  $ echo ddd > bar/file3
  $ hg commit -Aqm D
  $ hg push -q --to master_bookmark
  $ hg update -q .^
  $ hg subtree copy -r . --from-path foo --to-path bar
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg push -r . --to master_bookmark
  pushing rev 61c5ff2acb3a to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 1 changeset
  pushrebasing stack (416d3b39a0c6, 61c5ff2acb3a] (1 commit) to remote bookmark master_bookmark
  abort: Server error: Conflicts while pushrebasing: [PushrebaseConflict { left: MPath("bar/file3"), right: MPath("bar") }]
  [255]

Subtree copies can overwrite directories as long as there are no conflicts

  $ hg update -q master_bookmark
  $ hg subtree copy -r . --from-path foo --to-path bar
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg push -r . --to master_bookmark
  pushing rev fa91136eaad4 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 1 changeset
  pushrebasing stack (3f13bd9cda35, fa91136eaad4] (1 commit) to remote bookmark master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to fa91136eaad4
  $ diff -u foo bar

Subtree copies can be pushrebased with their own contents

  $ echo eee > foo/file2
  $ hg commit -qm E
  $ echo fff > foo/file2
  $ hg commit -qm F
  $ hg push -q --to master_bookmark
  $ hg update -q .^
  $ hg subtree copy -r . --from-path foo --to-path bar
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo ggg > bar/file2
  $ hg amend -q
  $ hg push -r . --to master_bookmark
  pushing rev 16b0ba258f0d to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 1 changeset
  pushrebasing stack (8af584e22863, 16b0ba258f0d] (1 commit) to remote bookmark master_bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to 7b61498bb1ca
  $ cat foo/file2
  fff
  $ cat bar/file2
  ggg
