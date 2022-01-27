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
  $ echo "test content" > test.txt
  $ hg commit -Aqm "add test.txt"
  $ ROOT_MFID_1=$(hg log -r . -T '{manifest}')
  $ hg cp test.txt copy.txt
  $ hg commit -Aqm "copy test.txt to test2.txt"
  $ ROOT_MFID_2=$(hg log -r . -T '{manifest}')

Blobimport test repo.
  $ cd ..
  $ blobimport repo-hg/.hg repo

Start up EdenAPI server.
  $ mononoke
  $ wait_for_mononoke

Create and send tree request.
  $ cat > keys << EOF
  > [
  >     ("", "$ROOT_MFID_1"),
  >     ("", "$ROOT_MFID_2")
  > ]
  > EOF

  $ cat > attrs << EOF
  > {
  >     "manifest_blob": True,
  >     "parents": True,
  >     "child_metadata": True
  > }
  > EOF

  $ hgedenapi debugapi -e trees -f keys -f attrs --sort
  [{"key": {"node": bin("15024c4dc4a27b572d623db342ae6a08d7f7adec"),
            "path": ""},
    "data": b"test.txt\0186cafa3319c24956783383dc44c5cbc68c5a0ca\n",
    "parents": None},
   {"key": {"node": bin("c8743b14e0789cc546125213c18a18d813862db5"),
            "path": ""},
    "data": b"copy.txt\017b8d4e3bafd4ec4812ad7c930aace9bf07ab033\ntest.txt\0186cafa3319c24956783383dc44c5cbc68c5a0ca\n",
    "parents": bin("15024c4dc4a27b572d623db342ae6a08d7f7adec")}]

