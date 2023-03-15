# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Start up EdenAPI server.
  $ setup_mononoke_config
  $ SCUBA="$TESTTMP/scuba.json"
  $ start_and_wait_for_mononoke_server --scuba-dataset "file://$SCUBA"

ignore the scuba logs logged while starting the server
  $ truncate -s 0 "$SCUBA"

List repos.
  $ sslcurl -s "https://localhost:$MONONOKE_SOCKET/edenapi/repos"
  {"repos":["repo"]} (no-eol)

  $ cat "$SCUBA" | summarize_scuba_json "EdenAPI.*" \
  >     .normal.log_tag .normal.http_method .normal.http_path \
  >     .int.poll_count .int.poll_time_us
  {
    "http_method": "GET",
    "http_path": "/repos",
    "log_tag": "EdenAPI Request Processed"
  }
