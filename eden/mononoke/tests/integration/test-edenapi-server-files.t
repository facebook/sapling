# Copyright (c) Facebook, Inc. and its affiliates.
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
    "aux_data": {"sha1": [79,
                          226,
                          184,
                          221,
                          18,
                          205,
                          156,
                          214,
                          164,
                          19,
                          234,
                          150,
                          12,
                          216,
                          192,
                          156,
                          37,
                          241,
                          149,
                          39],
                 "sha256": [161,
                            255,
                            240,
                            255,
                            239,
                            185,
                            234,
                            206,
                            114,
                            48,
                            194,
                            78,
                            80,
                            115,
                            31,
                            10,
                            145,
                            198,
                            47,
                            156,
                            239,
                            223,
                            231,
                            113,
                            33,
                            194,
                            246,
                            7,
                            18,
                            93,
                            255,
                            174],
                 "content_id": [136,
                                141,
                                207,
                                83,
                                58,
                                53,
                                76,
                                35,
                                228,
                                191,
                                103,
                                225,
                                173,
                                169,
                                132,
                                217,
                                107,
                                177,
                                8,
                                155,
                                12,
                                60,
                                3,
                                244,
                                194,
                                203,
                                119,
                                55,
                                9,
                                231,
                                170,
                                66],
                 "total_size": 13}}]

