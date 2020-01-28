# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup configuration. Unset a handful of variables that otherwise clutter Scuba logging.
  $ unset SMC_TIERS TW_TASK_ID TW_CANARY_ID TW_JOB_CLUSTER TW_JOB_USER TW_JOB_NAME
  $ export WIREPROTO_LOGGING_PATH="$TESTTMP/wireproto.json"
  $ BLOB_TYPE="blob_files" quiet default_setup

Make requests
  $ quiet hgmn  pull
  $ quiet hgmn up master_bookmark

Wait for requests to be logged
  $ wait_for_json_record_count "$WIREPROTO_LOGGING_PATH" 3
  $ jq -r .normal.command "$WIREPROTO_LOGGING_PATH"
  getbundle
  gettreepack
  getpackv1

Replay traffic
  $ fastreplay_log="$TESTTMP/fastreplay.json"
  $ quiet fastreplay --scuba-log-file "$fastreplay_log" < "$WIREPROTO_LOGGING_PATH"
  $ jq -r .normal.command  "$fastreplay_log" | grep -v null | sort | uniq
  getbundle
  getpackv1
  gettreepack
  $ jq -r .normal.log_tag  "$fastreplay_log" | grep Replay
  Replay Succeeded
  Replay Succeeded
  Replay Succeeded
  $ truncate -s 0 "$fastreplay_log"

Test a few more options
  $ quiet fastreplay --hash-validation-percentage 10  --scuba-log-file "$fastreplay_log" < "$WIREPROTO_LOGGING_PATH"
  $ jq -r .normal.log_tag  "$fastreplay_log" | grep Replay
  Replay Succeeded
  Replay Succeeded
  Replay Succeeded

Check logging structure
  $ grep "Replay Succeeded" "$fastreplay_log" | head -n 1 | jq .
  {
    "int": {
      "completion_time_us": *, (glob)
      "poll_count": *, (glob)
      "poll_time_us": *, (glob)
      "recorded_duration_us": *, (glob)
      "replay_response_size": *, (glob)
      "time": * (glob)
    },
    "normal": {
      "build_revision": *, (glob)
      "build_rule": *, (glob)
      "log_tag": "Replay Succeeded",
      "recorded_mononoke_session_id": *, (glob)
      "recorded_server": "mononoke",
      "reponame": "repo",
      "server_hostname": * (glob)
    }
  }

# Check that replaying with admission rate = 0 does not replay
  $ truncate -s 0 "$fastreplay_log"
  $ live_config="$TESTTMP/live.json"
  $ cat > "$live_config" << EOF
  > {
  >   "admission_rate": 0
  > }
  > EOF
  $ fastreplay  --live-config "file:${live_config}" --debug < "$WIREPROTO_LOGGING_PATH" 2>&1 | grep "not admitted"
  * Request was not admitted (glob)
  * Request was not admitted (glob)
  * Request was not admitted (glob)
