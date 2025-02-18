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
  $ setup_common_config blob_files

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

  $ hg update .^
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg subtree copy -r .^ --from-path foo --to-path bar
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls bar
  file1
  $ cat bar/file1
  aaa
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  subtree=[{"copies":[{"from_commit":"13445855d10c80bc6ef92e531c44430ea1101b6e","from_path":"foo","to_path":"bar"}],"v":1}]

  $ echo ddd > bar/file1
  $ hg commit -m D

  $ hg log -G -T '{node|short} {desc|firstline} {remotebookmarks}\n'
  @  fe5f3792bda1 D
  │
  o  a925cd481025 Subtree copy from 13445855d10c80bc6ef92e531c44430ea1101b6e
  │
  │ o  d55124608f34 C remote/master_bookmark
  ├─╯
  o  8aeb486cc22e B
  │
  o  13445855d10c A
  
  $ hg push -r . --to master_bookmark
  pushing rev fe5f3792bda1 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 2 commits for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 3 trees for upload
  edenapi: uploaded 3 trees
  edenapi: uploaded 2 changesets
  pushrebasing stack (8aeb486cc22e, fe5f3792bda1] (2 commits) to remote bookmark master_bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to c9f05026d4e6

Create another change that is not related
  $ echo other > other.txt
  $ hg commit -Am other
  adding other.txt
  $ hg push -q -r . --to master_bookmark

  $ hg update .^
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg subtree merge -r . --from-path bar --to-path foo
  computing merge base (timeout: 120 seconds)...
  merge base: 13445855d10c
  merging foo/file2 and bar/file1 to foo/file2
  warning: 1 conflicts while merging foo/file2! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ echo ddd > foo/file2
  $ hg resolve --mark foo/file2
  (no more unresolved files)
  $ hg commit -m "subtree merge"
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  subtree=[{"merges":[{"from_commit":"c9f05026d4e69a4908278a9d8e826559d1f4bed7","from_path":"bar","to_path":"foo"}],"v":1}]

  $ hg log -G -T '{node|short} {desc|firstline} {remotebookmarks}\n'
  @  a21822ff36fe subtree merge
  │
  │ o  1b811e71494e other remote/master_bookmark
  ├─╯
  o  c9f05026d4e6 D
  │
  o  416d3b39a0c6 Subtree copy from 13445855d10c80bc6ef92e531c44430ea1101b6e
  │
  o  d55124608f34 C
  │
  o  8aeb486cc22e B
  │
  o  13445855d10c A
  

  $ hg push -r . --to master_bookmark
  pushing rev a21822ff36fe to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 1 changeset
  pushrebasing stack (c9f05026d4e6, a21822ff36fe] (1 commit) to remote bookmark master_bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to acf4a4a7aa0a

  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  subtree=[{"merges":[{"from_commit":"c9f05026d4e69a4908278a9d8e826559d1f4bed7","from_path":"bar","to_path":"foo"}],"v":1}]
