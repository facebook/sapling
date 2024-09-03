# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setup_common_config
  $ setconfig experimental.edenapi-suffixquery=true

  $ start_and_wait_for_mononoke_server
  $ hg clone -q mono:repo repo
  $ cd repo
  $ drawdag << EOS
  > D # D/bar = zero\nuno\ntwo\n
  > |
  > C # C/bar = zero\none\ntwo\n (renamed from foo)
  > |
  > B # B/foo = one\ntwo\n
  > |
  > A # A/foo = one\n
  > EOS

Test suffix query output errors if commit not on server:
  $ hg debugapi -e suffix_query -i "{'Hg': '$(hg whereami)'}" -i "['.txt']" -i None
  abort: server responded 400 Bad Request for https://localhost:*/edenapi/repo/suffix_query: {"message":"CommitId not found: *","request_id":"*"}. Headers: { (glob)
      "x-request-id": "*", (glob)
      "content-type": "application/json",
      "x-load": "1",
      "server": "edenapi_server",
      "x-mononoke-host": * (glob)
      "date": * (glob)
      "content-length": "*", (glob)
  }
  [255]
API works:
  $ touch tmp.txt
  $ mkdir src
  $ touch src/rust.rs
  $ hg add tmp.txt
  $ hg add src/rust.rs
  $ hg commit -m "jkter"
  $ hg push -q --to master --create
  $ hg debugapi -e suffix_query -i "{'Hg': '$(hg whereami)'}" -i "['.txt']" -i None
  [{"file_path": "tmp.txt"}]
  $ hg debugapi -e suffix_query -i "{'Hg': '$(hg whereami)'}" -i "['.rs']" -i None
  [{"file_path": "src/rust.rs"}]
  $ hg debugapi -e suffix_query -i "{'Hg': '$(hg whereami)'}" -i "['.cpp']" -i None
  []
  $ touch src/nested.txt
  $ hg add src/nested.txt
  $ hg commit -m "mint"
  $ hg push -q --to master
  $ hg debugapi -e suffix_query -i "{'Hg': '$(hg whereami)'}" -i "['.txt']" -i "['src']"
  [{"file_path": "src/nested.txt"}]
