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
  > A-B-C-D-E-F
  > # modify: A x a_content
  > # modify: C x c_content
  > # modify: C A aa
  > # modify: D x d_content
  > # modify: E x e_content
  > # modify: F A af
  > EOF
  A=6d36fe0b5a1a231b995e54604fd703d3278dea4d
  B=79b9282e7253e88c76cdd445e07a0abc006b9dd2
  C=5e6285c22eb27ccf7c0e47c0e241abd33ddb1464
  D=96b264e8052b57c3c1c896d65645375230e8a516
  E=16911be50dca260bfef9d9dc6be5e1f8415d6c25
  F=5e8ea275292b062c658ecd10ec4e8d90159b3ff5

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
