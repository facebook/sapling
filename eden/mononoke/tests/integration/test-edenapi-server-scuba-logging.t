# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Start up EdenAPI server.
  $ SCUBA="$TESTTMP/scuba.json"
  $ setup_common_config
  $ mononoke --scuba-log-file "$SCUBA"
  $ wait_for_mononoke

Send a request

  $ ID1="1111111111111111111111111111111111111111"
  $ ID2="2222222222222222222222222222222222222222"
  $ cat > req <<EOF
  > [
  >  ("", "$ID1"),
  >  ("", "$ID2"),
  > ]
  > EOF

  $ hgedenapi debugapi -e files -f req --sort
  [{"key": {"node": bin("2222222222222222222222222222222222222222"),
            "path": ""},
    "result": {"Err": {"code": 0,
                       "message": "Key does not exist: Key { path: RepoPathBuf(\"\"), hgid: HgId(\"2222222222222222222222222222222222222222\") }"}}},
   {"key": {"node": bin("1111111111111111111111111111111111111111"),
            "path": ""},
    "result": {"Err": {"code": 0,
                       "message": "Key does not exist: Key { path: RepoPathBuf(\"\"), hgid: HgId(\"1111111111111111111111111111111111111111\") }"}}}]
