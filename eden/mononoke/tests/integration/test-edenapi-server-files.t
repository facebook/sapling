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
  $ TEST_FILENODE=$(hg manifest --debug | grep test.txt | awk '{print $1}')
  $ hg cp test.txt copy.txt
  $ hg commit -Aqm "copy test.txt to test2.txt"
  $ COPY_FILENODE=$(hg manifest --debug | grep copy.txt | awk '{print $1}')

Blobimport test repo.
  $ cd ..
  $ blobimport repo-hg/.hg repo

Start up EdenAPI server.
  $ setup_mononoke_config
  $ mononoke
  $ wait_for_mononoke

Create and send file request.
  $ cat > req << EOF
  > [{
  >   "key": {"path": "copy.txt", "node": "$COPY_FILENODE"},
  >   "attrs": {"aux_data": True, "content": True}
  > }]
  > EOF

Check files in response.
  $ hgedenapi debugapi -e filesattrs -f req
  [{"key": {"node": bin("17b8d4e3bafd4ec4812ad7c930aace9bf07ab033"),
            "path": "copy.txt"},
    "content": {"metadata": {"size": None,
                             "flags": None},
                "hg_file_blob": b"\x01\ncopy: test.txt\ncopyrev: 186cafa3319c24956783383dc44c5cbc68c5a0ca\n\x01\ntest content\n"},
    "parents": None,
    "aux_data": {"sha1": bin("4fe2b8dd12cd9cd6a413ea960cd8c09c25f19527"),
                 "sha256": bin("a1fff0ffefb9eace7230c24e50731f0a91c62f9cefdfe77121c2f607125dffae"),
                 "content_id": bin("888dcf533a354c23e4bf67e1ada984d96bb1089b0c3c03f4c2cb773709e7aa42"),
                 "total_size": 13}}]

