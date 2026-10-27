# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

An invalid prefix in a suffix query ('..' is not a valid repo path) is a
client error: the server answers 400 Bad Request instead of panicking.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setup_common_config

  $ testtool_drawdag -R repo --print-hg-hashes --no-default-files << EOF
  > A
  > # modify: A "foo.txt" "one\n"
  > # bookmark: A master_bookmark
  > EOF
  A=* (glob)
  $ start_and_wait_for_mononoke_server

  $ hg clone -q mono:repo repo
  $ cd repo
  $ hg up -q "$A"

A valid query works:
  $ hg debugapi -e suffix_query -i "{'Hg': '$(hg whereami)'}" -i "['.txt']" -i None
  [{"file_path": "foo.txt"}]

An invalid prefix is rejected with 400:
  $ hg debugapi -e suffix_query -i "{'Hg': '$(hg whereami)'}" -i "['.txt']" -i "['..']"
  abort: server responded 400 Bad Request for https://localhost:*/edenapi/repo/suffix_query: {"message":"*","request_id":"*"}. Headers: { (glob)
      "x-request-id": "*", (glob)
      "content-type": "application/json",
      "x-load": "1",
      "server": "edenapi_server",
      "x-mononoke-host": * (glob)
      "date": * (glob)
      "content-length": "*", (glob)
  }
  [255]

The server is still healthy afterwards:
  $ hg debugapi -e suffix_query -i "{'Hg': '$(hg whereami)'}" -i "['.txt']" -i None
  [{"file_path": "foo.txt"}]
