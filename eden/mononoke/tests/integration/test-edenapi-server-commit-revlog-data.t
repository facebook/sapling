# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Set up local hgrc and Mononoke config.
  $ setup_common_config
  $ cd $TESTTMP

Initialize test repo.
  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ setup_hg_server

Populate test repo
  $ echo "my commit message" > test.txt
  $ hg commit -Aqm "add test.txt"
  $ COMMIT_1=$(hg log -r . -T '{node}')
  $ hg cp test.txt copy.txt
  $ hg commit -Aqm "copy test.txt to test2.txt"
  $ COMMIT_2=$(hg log -r . -T '{node}')

Blobimport test repo.
  $ cd ..
  $ blobimport repo-hg/.hg repo

Start up EdenAPI server.
  $ mononoke
  $ wait_for_mononoke

Check response.
  $ hgedenapi debugapi -e commitdata -i "['$COMMIT_1','$COMMIT_2']"
  [{"hgid": bin("e83645968c8f2954b97a3c79ce5a6b90a464c54d"),
    "revlog_data": b"\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\09b8fa746094652af6be3a05047424c31a48c5fac\ntest\n0 0\ntest.txt\n\nadd test.txt"},
   {"hgid": bin("c7dcf24fab3a8ab956273fa40d5cc44bc26ec655"),
    "revlog_data": b"\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\xe86E\x96\x8c\x8f)T\xb9z<y\xceZk\x90\xa4d\xc5M815f6cad2ce1ccbf87151e2d7223c92899d9026c\ntest\n0 0\ncopy.txt\n\ncopy test.txt to test2.txt"}]

