# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig push.edenapi=true
  $ ENABLE_API_WRITES=1 BLOB_TYPE="blob_files" default_setup --scuba-dataset "file://$TESTTMP/log.json"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting
  starting Mononoke
  cloning repo in hg client 'repo2'

subtree copy and push

  $ hg up $C
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkdir foo
  $ echo aaa > foo/file1
  $ hg ci -qAm 'add foo/file1'
  $ hg mv foo/file1 foo/file2
  $ hg ci -m 'foo/file1 -> foo/file2'
  $ echo bbb >> foo/file2
  $ hg ci -m 'update foo/file2'
  $ hg subtree copy -r .^ --from-path foo --to-path bar
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls bar
  file2
  $ cat bar/file2
  aaa

  $ hg log -G -T '{node|short} {desc|firstline} {remotebookmarks}\n'
  @  6c8bfea34e65 Subtree copy from 8174a01c532cd975ecb875fb1556590dd776b29e
  │
  o  64a6d9b95dad update foo/file2
  │
  o  8174a01c532c foo/file1 -> foo/file2
  │
  o  4e1aaf1e01be add foo/file1
  │
  o  26805aba1e60 C default/master_bookmark
  │
  o  112478962961 B
  │
  o  426bada5c675 A
  
tofix: push should be succeeded after Mononoke support subtree copy metadata
  $ hg push -r . --to master_bookmark
  pushing rev 6c8bfea34e65 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 4 commits for upload
  edenapi: queue 3 files for upload
  edenapi: uploaded 3 files
  edenapi: queue 8 trees for upload
  edenapi: uploaded 8 trees
  edenapi: uploaded 3 changesets
  abort: failed to upload commits to server: ['6c8bfea34e6524c98e83ad375ad485320fa06b04']
  [255]

  $ rg "Incorrect copy info" $TESTTMP/log.json --no-filename | jq '.normal.edenapi_error'
  * Incorrect copy info: not found a file version foo/file1 2dce614a68fd6647ca187d760191a35d1cab54d8 the file bar/file2 b38f90c0ef9cb3c9f06668edc38e13c4c816d8cb was copied from" (glob)
