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

  $ hg subtree copy -r .^ --from-path foo --to-path bar
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls bar
  file2
  $ cat bar/file2
  bbb
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  subtree=[{"copies":[{"from_commit":"8aeb486cc22e0905ea4f23cf8b129c7546de26ac","from_path":"foo","to_path":"bar"}],"v":1}]

  $ hg log -G -T '{node|short} {desc|firstline} {remotebookmarks}\n'
  @  8feb31bbd259 Subtree copy from 8aeb486cc22e0905ea4f23cf8b129c7546de26ac
  │
  o  d55124608f34 C remote/master_bookmark
  │
  o  8aeb486cc22e B
  │
  o  13445855d10c A
  
  $ hg push -r . --to master_bookmark
  pushing rev 8feb31bbd259 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 1 changeset
  pushrebasing stack (d55124608f34, 8feb31bbd259] (1 commit) to remote bookmark master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to 8feb31bbd259
