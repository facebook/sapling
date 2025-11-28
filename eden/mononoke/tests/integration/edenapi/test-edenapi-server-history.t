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
  $ hginit_treemanifest repo
  $ cd repo

Create initial commit with testtool
  $ testtool_drawdag -R repo << EOF
  > base
  > # bookmark: base master_bookmark
  > EOF
  base=* (glob)

  $ cd "$TESTTMP"

Start up SaplingRemoteAPI server.
  $ setup_mononoke_config
  $ start_and_wait_for_mononoke_server

Create commits using hg and push to Mononoke
  $ hg clone mono:repo client -q
  $ cd client

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

Push commits to Mononoke
  $ hg push -r . --to master_bookmark -q

Get filenodes for API request
  $ TEST_FILENODE=$(hg manifest --debug | grep test.txt | awk '{print $1}')
  $ COPY_FILENODE=$(hg manifest --debug | grep copy.txt | awk '{print $1}')

Create and send file data request.
  $ cat > req << EOF
  > [
  >     ("test.txt", "$TEST_FILENODE"),
  >     ("copy.txt", "$COPY_FILENODE")
  > ]
  > EOF

  $ mononoke_admin derived-data -R repo derive -T filenodes -B master_bookmark
  $ hg debugapi mono:repo -e history -f req --sort
  [{"key": {"node": bin("17b8d4e3bafd4ec4812ad7c930aace9bf07ab033"),
            "path": "copy.txt"},
    "nodeinfo": {"parents": [{"node": bin("186cafa3319c24956783383dc44c5cbc68c5a0ca"),
                              "path": "test.txt"},
                             {"node": bin("0000000000000000000000000000000000000000"),
                              "path": ""}],
                 "linknode": bin("8ae4731a3953713c4a7c03663f28d615aebff878")}},
   {"key": {"node": bin("672343a6daad357b926cd84a5a44a011ad029e5f"),
            "path": "copy.txt"},
    "nodeinfo": {"parents": [{"node": bin("17b8d4e3bafd4ec4812ad7c930aace9bf07ab033"),
                              "path": "copy.txt"},
                             {"node": bin("0000000000000000000000000000000000000000"),
                              "path": ""}],
                 "linknode": bin("023ad793609257f0812cb444cd653d0b24785836")}},
   {"key": {"node": bin("b6fe30270546463f3630fd41fec2cd113e7a8acf"),
            "path": "test.txt"},
    "nodeinfo": {"parents": [{"node": bin("186cafa3319c24956783383dc44c5cbc68c5a0ca"),
                              "path": "test.txt"},
                             {"node": bin("0000000000000000000000000000000000000000"),
                              "path": ""}],
                 "linknode": bin("023ad793609257f0812cb444cd653d0b24785836")}},
   {"key": {"node": bin("596c909aab726d7f8b3766795239cd20ede8e125"),
            "path": "test.txt"},
    "nodeinfo": {"parents": [{"node": bin("b6fe30270546463f3630fd41fec2cd113e7a8acf"),
                              "path": "test.txt"},
                             {"node": bin("0000000000000000000000000000000000000000"),
                              "path": ""}],
                 "linknode": bin("89104e8c826c2a9df135001de239b0447da94867")}},
   {"key": {"node": bin("186cafa3319c24956783383dc44c5cbc68c5a0ca"),
            "path": "test.txt"},
    "nodeinfo": {"parents": [{"node": bin("0000000000000000000000000000000000000000"),
                              "path": ""},
                             {"node": bin("0000000000000000000000000000000000000000"),
                              "path": ""}],
                 "linknode": bin("b2b730938b82e2edcb09957d2610f1ebc77d4d43")}}]
