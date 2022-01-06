# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Set up local hgrc and Mononoke config.
  $ setup_common_config
  $ setup_configerator_configs
  $ cd $TESTTMP

Initialize test repo.
  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ setup_hg_server

Populate test repo
  $ echo "test content" > test.txt
  $ hg commit -Aqm "add test.txt"
  $ hg cp test.txt copy.txt
  $ hg commit -Aqm "copy test.txt to test2.txt"
  $ echo "line 2" >> test.txt
  $ echo "line 2" >> copy.txt
  $ hg commit -qm "add line 2 to test files"
  $ echo "line 3" >> test.txt
  $ echo "line 3" >> test2.txt
  $ hg commit -qm "add line 3 to test files"
  $ TEST_FILENODE=$(hg manifest --debug | grep test.txt | awk '{print $1}')
  $ COPY_FILENODE=$(hg manifest --debug | grep copy.txt | awk '{print $1}')

Blobimport test repo.
  $ cd ..
  $ blobimport repo-hg/.hg repo

Start up EdenAPI server.
  $ mononoke
  $ wait_for_mononoke

Create and send file data request.
  $ cat > req << EOF
  > [
  >     ("test.txt", "$TEST_FILENODE"),
  >     ("copy.txt", "$COPY_FILENODE")
  > ]
  > EOF

  $ hgedenapi debugapi -e history -f req
  [{"key": {"node": bin("596c909aab726d7f8b3766795239cd20ede8e125"),
            "path": "test.txt"},
    "nodeinfo": {"parents": [{"node": bin("b6fe30270546463f3630fd41fec2cd113e7a8acf"),
                              "path": "test.txt"},
                             {"node": bin("0000000000000000000000000000000000000000"),
                              "path": ""}],
                 "linknode": bin("4af0b091e704c445e593c61b40564872773e64b3")}},
   {"key": {"node": bin("b6fe30270546463f3630fd41fec2cd113e7a8acf"),
            "path": "test.txt"},
    "nodeinfo": {"parents": [{"node": bin("186cafa3319c24956783383dc44c5cbc68c5a0ca"),
                              "path": "test.txt"},
                             {"node": bin("0000000000000000000000000000000000000000"),
                              "path": ""}],
                 "linknode": bin("6f445033ece95e6f81f0fd93cb0db7e29862888a")}},
   {"key": {"node": bin("186cafa3319c24956783383dc44c5cbc68c5a0ca"),
            "path": "test.txt"},
    "nodeinfo": {"parents": [{"node": bin("0000000000000000000000000000000000000000"),
                              "path": ""},
                             {"node": bin("0000000000000000000000000000000000000000"),
                              "path": ""}],
                 "linknode": bin("f91e155a86e1b909d99174818a2f98de2c128c59")}},
   {"key": {"node": bin("672343a6daad357b926cd84a5a44a011ad029e5f"),
            "path": "copy.txt"},
    "nodeinfo": {"parents": [{"node": bin("17b8d4e3bafd4ec4812ad7c930aace9bf07ab033"),
                              "path": "copy.txt"},
                             {"node": bin("0000000000000000000000000000000000000000"),
                              "path": ""}],
                 "linknode": bin("6f445033ece95e6f81f0fd93cb0db7e29862888a")}},
   {"key": {"node": bin("17b8d4e3bafd4ec4812ad7c930aace9bf07ab033"),
            "path": "copy.txt"},
    "nodeinfo": {"parents": [{"node": bin("186cafa3319c24956783383dc44c5cbc68c5a0ca"),
                              "path": "test.txt"},
                             {"node": bin("0000000000000000000000000000000000000000"),
                              "path": ""}],
                 "linknode": bin("507881746c0f2eb0c6599fc8e4840d7cf45dcdbe")}}]
