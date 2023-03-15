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
  $ testtool_drawdag -R repo --print-hg-hashes <<'EOF'
  > A-B-C
  > EOF
  A=20ca2a4749a439b459125ef0f6a4f26e88ee7538
  B=80521a640a0c8f51dcc128c2658b224d595840ac
  C=d3b399ca8757acdb81c3681b052eb978db6768d8


ignore the scuba logs logged while starting the server
  $ truncate -s 0 "$SCUBA"

  $ setup_common_hg_configs

Run a few requests that use different codepaths for logging server-side
  $ sslcurl -s "https://localhost:$MONONOKE_SOCKET/edenapi/repos"
  {"repos":["repo"]} (no-eol)

  $ hgedenapi debugapi -e uploadfilecontents -i '[({"Sha1":"03cfd743661f07975fa2f1220c5194cbaff48451"}, b"abc\n")]' > /dev/null
  $ hgedenapi debugapi -e commitgraph -i "['$C']" -i "['$A']" --sort > /dev/null


  $ cat "$SCUBA" | summarize_scuba_json "EdenAPI.*" \
  >     .normal.log_tag .normal.http_method .normal.http_path \
  >     .int.poll_count .int.poll_time_us \
  >     .int.max_poll_time_us
  {
    "http_method": "GET",
    "http_path": "/repos",
    "log_tag": "EdenAPI Request Processed",
    "max_poll_time_us": \d*, (re)
    "poll_count": \d*, (re)
    "poll_time_us": \d* (re)
  }
  {
    "http_method": "POST",
    "http_path": "/repo/lookup",
    "log_tag": "EdenAPI Request Processed",
    "max_poll_time_us": \d*, (re)
    "poll_count": \d*, (re)
    "poll_time_us": \d* (re)
  }
  {
    "http_method": "PUT",
    "http_path": "/repo/upload/file/sha1/03cfd743661f07975fa2f1220c5194cbaff48451",
    "log_tag": "EdenAPI Request Processed",
    "max_poll_time_us": \d*, (re)
    "poll_count": \d*, (re)
    "poll_time_us": \d* (re)
  }
  {
    "http_method": "POST",
    "http_path": "/repo/commit/graph",
    "log_tag": "EdenAPI Request Processed",
    "max_poll_time_us": \d*, (re)
    "poll_count": \d*, (re)
    "poll_time_us": \d* (re)
  }
