# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setup_mononoke_config
  $ setup_common_config
  $ testtool_drawdag -R repo --derive-all << EOF
  > A-B-C
  > # bookmark: C heads/main
  > # modify: A a "a"
  > # author_date: A "1970-01-01T01:00:00+00:00"
  > # author: A test
  > EOF
  A=546ab8adb92af7ef882231ea89d5f3d6d1d0345f761aa7b3a25ff08f25aa0e85
  B=2ed9e9a92593a45b058ea55831f878cc2fcae8e3da7586dcfdb803966bd8a849
  C=a6c13a9f21601141386841ed5cb7d78666e026d215af303a6bf96ebd8db6385d
  $ start_and_wait_for_mononoke_server

# Check that file exists
  $ hg debugapi mono:repo -e blame -i "[{'path': 'A', 'node': 'abba944e59f369b1aea356d1609356a795459436'}]"
  [{"data": {"Ok": {"paths": ["A"],
                    "commits": [bin("ea0571263ad61e03c27820b572efec44de47f636")],
                    "line_ranges": [{"line_count": 1,
                                     "path_index": 0,
                                     "line_offset": 0,
                                     "commit_index": 0,
                                     "origin_line_offset": 0}]}},
    "file": {"node": bin("abba944e59f369b1aea356d1609356a795459436"),
             "path": "A"}}]

# Test with slapigit with git sha1 input
  $ hg --config remotefilelog.reponame=repo --config edenapi.url=https://localhost:$MONONOKE_SOCKET/slapigit/ debugapi -e blame -i "[{'path': 'A', 'node': 'ca5c1860d51d7cfbc1102f5d6aa1cfe6e44aeeff'}]"
  [{"data": {"Ok": {"paths": ["A"],
                    "commits": [bin("ca5c1860d51d7cfbc1102f5d6aa1cfe6e44aeeff")],
                    "line_ranges": [{"line_count": 1,
                                     "path_index": 0,
                                     "line_offset": 0,
                                     "commit_index": 0,
                                     "origin_line_offset": 0}]}},
    "file": {"node": bin("ca5c1860d51d7cfbc1102f5d6aa1cfe6e44aeeff"),
             "path": "A"}}]
