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
  copying foo to bar
  $ ls bar
  file1
  $ cat bar/file1
  aaa
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  subtree=[{"deepcopies":[{"from_commit":"13445855d10c80bc6ef92e531c44430ea1101b6e","from_path":"foo","to_path":"bar"}],"v":1}]

  $ echo ddd > bar/file1
  $ hg commit -m D

  $ hg log -G -T '{node|short} {desc|firstline} {remotebookmarks}\n'
  @  77195f886fcc D
  │
  o  88f76f29ed1a Subtree copy from 13445855d10c80bc6ef92e531c44430ea1101b6e
  │
  │ o  d55124608f34 C remote/master_bookmark
  ├─╯
  o  8aeb486cc22e B
  │
  o  13445855d10c A
  
  $ hg push -r . --to master_bookmark
  pushing rev 77195f886fcc to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 2 commits for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 3 trees for upload
  edenapi: uploaded 3 trees
  edenapi: uploaded 2 changesets
  pushrebasing stack (8aeb486cc22e, 77195f886fcc] (2 commits) to remote bookmark master_bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to 829e1f511ab2

Create another change that is not related
  $ echo other > other.txt
  $ hg commit -Am other
  adding other.txt
  $ hg push -q -r . --to master_bookmark

  $ hg update .^
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg subtree merge -r . --from-path bar --to-path foo
  searching for merge base ...
  found the last subtree copy commit ca8bcf7d3251
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
  subtree=[{"merges":[{"from_commit":"829e1f511ab278f9774b0940030a0d35485242e0","from_path":"bar","to_path":"foo"}],"v":1}]

  $ hg log -G -T '{node|short} {desc|firstline} {remotebookmarks}\n'
  @  965ce825305a subtree merge
  │
  │ o  5f22b1ec3108 other remote/master_bookmark
  ├─╯
  o  829e1f511ab2 D
  │
  o  ca8bcf7d3251 Subtree copy from 13445855d10c80bc6ef92e531c44430ea1101b6e
  │
  o  d55124608f34 C
  │
  o  8aeb486cc22e B
  │
  o  13445855d10c A
  

  $ hg push -r . --to master_bookmark
  pushing rev 965ce825305a to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 1 changeset
  pushrebasing stack (829e1f511ab2, 965ce825305a] (1 commit) to remote bookmark master_bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to ac71e5804398

  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  subtree=[{"merges":[{"from_commit":"829e1f511ab278f9774b0940030a0d35485242e0","from_path":"bar","to_path":"foo"}],"v":1}]
