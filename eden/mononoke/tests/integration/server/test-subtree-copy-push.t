# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig push.edenapi=true
  $ cat > $TESTTMP/subtree.py <<EOF
  > from sapling.commands import subtree
  > def extsetup(ui):
  >     subtree.COPY_REUSE_TREE = True
  > EOF
  $ setconfig extensions.subtreecopyreusetree=$TESTTMP/subtree.py
  $ BLOB_TYPE="blob_files" default_setup --scuba-log-file "$TESTTMP/log.json"
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  starting Mononoke
  cloning repo in hg client 'repo2'

subtree copy and push
  $ hg up master_bookmark
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkdir foo
  $ echo aaa > foo/file1
  $ hg ci -qAm 'add foo/file1'
  $ hg mv foo/file1 foo/file2
  $ hg ci -m 'foo/file1 -> foo/file2'
  $ echo bbb >> foo/file2
  $ hg ci -m 'update foo/file2'
  $ hg push -r . --to master_bookmark -q
  $ hg subtree copy -r .^ --from-path foo --to-path bar
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls bar
  file2
  $ cat bar/file2
  aaa
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  test_subtree=[{"copies":[{"from_commit":"47a1c68b921ca59adb1975d5486c2e00f6fbb9a0","from_path":"foo","to_path":"bar"}],"v":1}]

  $ hg log -G -T '{node|short} {desc|firstline} {remotebookmarks}\n'
  @  67dba3575ef5 Subtree copy from 47a1c68b921ca59adb1975d5486c2e00f6fbb9a0
  │
  o  ddfa87816335 update foo/file2 remote/master_bookmark
  │
  o  47a1c68b921c foo/file1 -> foo/file2
  │
  o  d350b243c628 add foo/file1
  │
  o  d3b399ca8757 C
  │
  o  80521a640a0c B
  │
  o  20ca2a4749a4 A
  
tofix: push should be succeeded after Mononoke support subtree copy metadata
  $ hg push -r . --to master_bookmark
  pushing rev 67dba3575ef5 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 0 changesets
  abort: failed to upload commits to server: ['67dba3575ef580491cf2544e2f0393b3812f7685']
  [255]

  $ rg "Incorrect copy info" $TESTTMP/log.json --no-filename | jq '.normal.edenapi_error'
  * Incorrect copy info: not found a file version foo/file1 2dce614a68fd6647ca187d760191a35d1cab54d8 the file bar/file2 b38f90c0ef9cb3c9f06668edc38e13c4c816d8cb was copied from" (glob)
