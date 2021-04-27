# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Start up EdenAPI server.
  $ SCUBA="$TESTTMP/scuba.json"
  $ setup_mononoke_config
  $ setup_configerator_configs
  $ start_edenapi_server --scuba-log-file "$SCUBA"

Send a request

  $ ID1="1111111111111111111111111111111111111111"
  $ ID2="2222222222222222222222222222222222222222"
  $ edenapi_make_req file > req.cbor <<EOF
  > {
  >   "keys": [
  >     ["", "$ID1"],
  >     ["", "$ID2"]
  >   ]
  > }
  > EOF
  Reading from stdin
  Generated request: WireFileRequest {
      keys: [
          WireKey {
              path: WireRepoPathBuf(
                  "",
              ),
              hgid: WireHgId("1111111111111111111111111111111111111111"),
          },
          WireKey {
              path: WireRepoPathBuf(
                  "",
              ),
              hgid: WireHgId("2222222222222222222222222222222222222222"),
          },
      ],
  }

  $ sslcurl -s "$EDENAPI_URI/repo/files" -d@req.cbor > res.cbor

Read the response, it should be empty

  $ edenapi_read_res file ls res.cbor
  Reading from file: "res.cbor"

Check the logging

  $ wait_for_json_record_count "$SCUBA" 1

  $ jq -r .normal.edenapi_error < "$SCUBA"
  Key does not exist: Key { path: RepoPathBuf(""), hgid: HgId("1111111111111111111111111111111111111111") }
  $ jq -r .int.edenapi_error_count < "$SCUBA"
  2
