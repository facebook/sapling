# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setup_common_config
  $ setconfig experimental.edenapi-blame=true

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

Errors are propagated:
  $ hg debugapi -e blame -i "[{'path': 'bar', 'node': '$D'}]"
  [{"data": {"Err": {"code": 0,
                     "message": "HgId not found: e9ace545f925b6f62ae34087895fdc950d168e5f"}},
    "file": {"node": bin("e9ace545f925b6f62ae34087895fdc950d168e5f"),
             "path": "bar"}}]

Fall back gracefully if edenapi not configured:
  $ hg blame -cldqf bar -r $D
  1ac4b616a32d 1970-01-01 bar:1: zero
  e9ace545f925 1970-01-01 bar:2: uno
  4b86660b0697 1970-01-01 foo:2: two

Fall back if server doesn't have commits:
  $ hg blame -cldqf bar -r $D
  1ac4b616a32d 1970-01-01 bar:1: zero
  e9ace545f925 1970-01-01 bar:2: uno
  4b86660b0697 1970-01-01 foo:2: two

Server has commits - use edenapi blame data:
  $ hg push -q -r $D --to master --create

  $ EDENSCM_LOG=edenapi::client=info hg blame -cldqf bar -r $D
   INFO edenapi::client: Blaming 1 file(s)
  1ac4b616a32d 1970-01-01 bar:1: zero
  e9ace545f925 1970-01-01 bar:2: uno
  4b86660b0697 1970-01-01 foo:2: two

Works with "wdir()" for unchanged files:
  $ hg go -q $D
  $ EDENSCM_LOG=edenapi::client=info hg blame -cldqf bar -r 'wdir()'
   INFO edenapi::client: Blaming 1 file(s)
  1ac4b616a32d  1970-01-01 bar:1: zero
  e9ace545f925  1970-01-01 bar:2: uno
  4b86660b0697  1970-01-01 foo:2: two

But doesn't work if file is dirty:
  $ echo dirty >> bar
  $ EDENSCM_LOG=edenapi::client=info hg blame -cldqf bar -r 'wdir()'
  1ac4b616a32d  1970-01-01 bar:1: zero
  e9ace545f925  1970-01-01 bar:2: uno
  4b86660b0697  1970-01-01 foo:2: two
  e9ace545f925+ ********** bar:4: dirty (glob)

Peek at what the data looks like:
  $ hg debugapi -e blame -i "[{'path': 'bar', 'node': '$D'}]"
  [{"data": {"Ok": {"paths": ["foo",
                              "bar"],
                    "commits": [bin("1ac4b616a32d09428a015bf6a11ccbd1c1410aad"),
                                bin("e9ace545f925b6f62ae34087895fdc950d168e5f"),
                                bin("4b86660b06977d770e191e5d454b6b2f2ca14818")],
                    "line_ranges": [{"line_count": 1,
                                     "path_index": 1,
                                     "line_offset": 0,
                                     "commit_index": 0,
                                     "origin_line_offset": 0},
                                    {"line_count": 1,
                                     "path_index": 1,
                                     "line_offset": 1,
                                     "commit_index": 1,
                                     "origin_line_offset": 1},
                                    {"line_count": 1,
                                     "path_index": 0,
                                     "line_offset": 2,
                                     "commit_index": 2,
                                     "origin_line_offset": 1}]}},
    "file": {"node": bin("e9ace545f925b6f62ae34087895fdc950d168e5f"),
             "path": "bar"}}]
