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
  copying foo to bar
  $ ls bar
  file1
  $ cat bar/file1
  aaa
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  subtree=[{"deepcopies":[{"from_commit":"13445855d10c80bc6ef92e531c44430ea1101b6e","from_path":"foo","to_path":"bar"}],"v":1}]

  $ hg log -G -T '{node|short} {desc|firstline} {remotebookmarks}\n'
  @  88f76f29ed1a Subtree copy from 13445855d10c80bc6ef92e531c44430ea1101b6e
  │
  │ o  d55124608f34 C remote/master_bookmark
  ├─╯
  o  8aeb486cc22e B
  │
  o  13445855d10c A
  
  $ hg push -r . --to master_bookmark
  pushing rev 88f76f29ed1a to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (8aeb486cc22e, 88f76f29ed1a] (1 commit) to remote bookmark master_bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to ca8bcf7d3251

  $ cat bar/file1
  aaa

Subtree copies should conflict with other subtree copies when pushrebasing.

  $ hg update -q .^
  $ hg subtree copy -r . --from-path foo --to-path bar
  copying foo to bar
  $ ls bar
  file2
  $ hg push -r . --to master_bookmark
  pushing rev 8ecbd0bd2240 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 1 changeset
  pushrebasing stack (d55124608f34, 8ecbd0bd2240] (1 commit) to remote bookmark master_bookmark
  abort: Server error: Conflicts while pushrebasing: [PushrebaseConflict { left: MPath("bar"), right: MPath("bar") }]
  [255]

Subtree copies should conflict with changes made to the destination (even if they are other files)

  $ hg update -q master_bookmark
  $ echo ddd > bar/file3
  $ hg commit -Aqm D
  $ hg push -q --to master_bookmark
  $ hg update -q .^
  $ hg subtree copy -r . --from-path foo --to-path bar --force
  removing bar/file1
  copying foo to bar
  $ hg push -r . --to master_bookmark
  pushing rev 60542fee7765 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 1 changeset
  pushrebasing stack (ca8bcf7d3251, 60542fee7765] (1 commit) to remote bookmark master_bookmark
  abort: Server error: Conflicts while pushrebasing: [PushrebaseConflict { left: MPath("bar/file3"), right: MPath("bar") }]
  [255]

Subtree copies can overwrite directories as long as there are no conflicts

  $ hg update -q master_bookmark
  $ hg subtree copy -r . --from-path foo --to-path bar --force
  removing bar/file1
  removing bar/file3
  copying foo to bar
  $ hg push -r . --to master_bookmark
  pushing rev d5d06a3000a2 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 1 changeset
  pushrebasing stack (ed89d1499167, d5d06a3000a2] (1 commit) to remote bookmark master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to d5d06a3000a2
  $ diff -u foo bar

Subtree copies can be pushrebased with their own contents and commits stacked on top

  $ echo eee > foo/file2
  $ hg commit -qm E
  $ echo fff > foo/file2
  $ hg commit -qm F
  $ hg push -q --to master_bookmark
  $ hg update -q .^
  $ hg subtree copy -r . --from-path foo --to-path bar --force
  removing bar/file2
  copying foo to bar
  $ echo ggg > bar/file2
  $ hg amend -q
  $ echo hhh >> bar/file2
  $ hg commit -qm H
  $ hg push -r . --to master_bookmark
  pushing rev 69cecf474522 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 2 commits for upload
  edenapi: queue 2 files for upload
  edenapi: uploaded 2 files
  edenapi: queue 4 trees for upload
  edenapi: uploaded 4 trees
  edenapi: uploaded 2 changesets
  pushrebasing stack (7c161b80faaa, 69cecf474522] (2 commits) to remote bookmark master_bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to 2cb798c4a29b
  $ cat foo/file2
  fff
  $ cat bar/file2
  ggg
  hhh
