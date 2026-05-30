# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Set up local hgrc and Mononoke config
  $ setup_common_config
  $ setup_configerator_configs
  $ cd $TESTTMP

Populate test repo
  $ testtool_drawdag -R repo --print-hg-hashes << EOF
  > A-B-C-D-E-F-G-H-I
  > # modify: A x a_content
  > # modify: C x c_content
  > # modify: C A aa
  > # modify: D x d_content
  > # modify: E x e_content
  > # modify: F A af
  > # delete: G x
  > # modify: H x h_content
  > # delete: I A
  > EOF
  A=6d36fe0b5a1a231b995e54604fd703d3278dea4d
  B=79b9282e7253e88c76cdd445e07a0abc006b9dd2
  C=5e6285c22eb27ccf7c0e47c0e241abd33ddb1464
  D=96b264e8052b57c3c1c896d65645375230e8a516
  E=16911be50dca260bfef9d9dc6be5e1f8415d6c25
  F=5e8ea275292b062c658ecd10ec4e8d90159b3ff5
  G=895fd9adde7927a9a78a6e8b5b9d48588d9a7860
  H=05f2b6a904ac3127f16b5b1a073f04a0a8460114
  I=269963e235f635e23caf1d5216b98e0b02fc39b2

Start up SaplingRemoteAPI server
  $ start_and_wait_for_mononoke_server

Query history with a wrong hash
  $ hg debugapi mono:repo -e path_history -i "'abc00000000000000000'" -i "['x']" -i None -i "[]" --sort
  [{"path": "x",
    "entries": {"Err": {"code": 0,
                        "message": "HgId not found: 6162633030303030303030303030303030303030"}}}]

Query history without limit or cursor
  $ hg debugapi mono:repo -e path_history -i "'$F'" -i "['x', 'A']" -i None -i "[]" --sort
  [{"path": "A",
    "entries": {"Ok": {"entries": [{"commit": bin("5e8ea275292b062c658ecd10ec4e8d90159b3ff5")},
                                   {"commit": bin("5e6285c22eb27ccf7c0e47c0e241abd33ddb1464")},
                                   {"commit": bin("6d36fe0b5a1a231b995e54604fd703d3278dea4d")}],
                       "has_more": False,
                       "next_commits": []}}},
   {"path": "x",
    "entries": {"Ok": {"entries": [{"commit": bin("16911be50dca260bfef9d9dc6be5e1f8415d6c25")},
                                   {"commit": bin("96b264e8052b57c3c1c896d65645375230e8a516")},
                                   {"commit": bin("5e6285c22eb27ccf7c0e47c0e241abd33ddb1464")},
                                   {"commit": bin("6d36fe0b5a1a231b995e54604fd703d3278dea4d")}],
                       "has_more": False,
                       "next_commits": []}}}]

Query history with limit 2
  $ hg debugapi mono:repo -e path_history -i "'$F'" -i "['x', 'A']" -i 2 -i "[]" --sort
  [{"path": "A",
    "entries": {"Ok": {"entries": [{"commit": bin("5e8ea275292b062c658ecd10ec4e8d90159b3ff5")},
                                   {"commit": bin("5e6285c22eb27ccf7c0e47c0e241abd33ddb1464")}],
                       "has_more": True,
                       "next_commits": [[bin("6d36fe0b5a1a231b995e54604fd703d3278dea4d"),
                                         None]]}}},
   {"path": "x",
    "entries": {"Ok": {"entries": [{"commit": bin("16911be50dca260bfef9d9dc6be5e1f8415d6c25")},
                                   {"commit": bin("96b264e8052b57c3c1c896d65645375230e8a516")}],
                       "has_more": True,
                       "next_commits": [[bin("5e6285c22eb27ccf7c0e47c0e241abd33ddb1464"),
                                         None]]}}}]

Query history with limit 1 and cursor from the previous query
  $ hg debugapi mono:repo -e path_history -i "'$F'" -i "['x', 'A']" -i 1 -i "[{'path': 'x', 'starting_commits': [('5e6285c22eb27ccf7c0e47c0e241abd33ddb1464', None)]}, {'path': 'A', 'starting_commits': [('6d36fe0b5a1a231b995e54604fd703d3278dea4d', None)]}]" --sort
  [{"path": "A",
    "entries": {"Ok": {"entries": [{"commit": bin("6d36fe0b5a1a231b995e54604fd703d3278dea4d")}],
                       "has_more": False,
                       "next_commits": []}}},
   {"path": "x",
    "entries": {"Ok": {"entries": [{"commit": bin("5e6285c22eb27ccf7c0e47c0e241abd33ddb1464")}],
                       "has_more": True,
                       "next_commits": [[bin("6d36fe0b5a1a231b995e54604fd703d3278dea4d"),
                                         None]]}}}]

Query history after deletion
  $ hg debugapi mono:repo -e path_history -i "'$I'" -i "['x', 'A']" -i None -i "[]" --sort
  [{"path": "A",
    "entries": {"Ok": {"entries": [{"commit": bin("269963e235f635e23caf1d5216b98e0b02fc39b2")},
                                   {"commit": bin("5e8ea275292b062c658ecd10ec4e8d90159b3ff5")},
                                   {"commit": bin("5e6285c22eb27ccf7c0e47c0e241abd33ddb1464")},
                                   {"commit": bin("6d36fe0b5a1a231b995e54604fd703d3278dea4d")}],
                       "has_more": False,
                       "next_commits": []}}},
   {"path": "x",
    "entries": {"Ok": {"entries": [{"commit": bin("05f2b6a904ac3127f16b5b1a073f04a0a8460114")}],
                       "has_more": False,
                       "next_commits": []}}}]

Query deleted path history with limit 2
  $ hg debugapi mono:repo -e path_history -i "'$I'" -i "['A']" -i 2 -i "[]" --sort
  [{"path": "A",
    "entries": {"Ok": {"entries": [{"commit": bin("269963e235f635e23caf1d5216b98e0b02fc39b2")},
                                   {"commit": bin("5e8ea275292b062c658ecd10ec4e8d90159b3ff5")}],
                       "has_more": True,
                       "next_commits": [[bin("5e6285c22eb27ccf7c0e47c0e241abd33ddb1464"),
                                         None]]}}}]

Query deleted path history with cursor from the previous query
  $ hg debugapi mono:repo -e path_history -i "'$I'" -i "['A']" -i 2 -i "[{'path': 'A', 'starting_commits': [('5e6285c22eb27ccf7c0e47c0e241abd33ddb1464', None)]}]" --sort
  [{"path": "A",
    "entries": {"Ok": {"entries": [{"commit": bin("5e6285c22eb27ccf7c0e47c0e241abd33ddb1464")},
                                   {"commit": bin("6d36fe0b5a1a231b995e54604fd703d3278dea4d")}],
                       "has_more": False,
                       "next_commits": []}}}]
