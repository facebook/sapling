# Copyright (c) Meta Platforms, Inc. and affiliates.
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
  $ quiet fastreplay --with-dynamic-observability=true --scuba-log-file "$fastreplay_log" < "$WIREPROTO_LOGGING_PATH"
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
  $ quiet fastreplay --with-dynamic-observability=true --hash-validation-percentage 10  --scuba-log-file "$fastreplay_log" < "$WIREPROTO_LOGGING_PATH"
  $ jq -r .normal.log_tag  "$fastreplay_log" | grep Replay
  Replay Succeeded
  Replay Succeeded
  Replay Succeeded

Check logging structure
  $ grep "Replay Succeeded" "$fastreplay_log" | grep gettreepack | head -n 1 | format_single_scuba_sample
  {
    "int": {
      "completion_time_us": *, (glob)
      "poll_count": *, (glob)
      "poll_time_us": *, (glob)
      "recorded_duration_us": *, (glob)
      "replay_delay_s": * (glob)
      "replay_response_size": *, (glob)
      "sample_rate": 1,
      "seq": *, (glob)
      "time": * (glob)
    },
    "normal": {
      "command": "gettreepack",
      "command_args": "[{\"basemfnodes\":\"\",\"depth\":\"1\",\"directories\":\",\",\"mfnodes\":\"7c9b4fd8b49377e2fead2e9610bb8db910a98c53\",\"rootdir\":\"\"}]",
      "log_tag": "Replay Succeeded",
      "recorded_mononoke_session_id": *, (glob)
      "recorded_server": "mononoke",
      "reponame": "repo"
    }
  }

Check repo client Scuba logging
  $ grep "Gettreepack Params" "$fastreplay_log" | jq -r .int.gettreepack_mfnodes
  1
  $ grep "Getpack Params" "$fastreplay_log" | jq -r .int.getpack_paths
  3

# Check that replaying with admission rate = 0 does not replay
  $ truncate -s 0 "$fastreplay_log"
  $ live_config="${LOCAL_CONFIGERATOR_PATH}/live.json"
  $ cat > "$live_config" << EOF
  > {
  >   "admission_rate": 0,
  >   "max_concurrency": 10,
  >   "scuba_sampling_target": 1,
  >   "skipped_repos": []
  > }
  > EOF
  $ fastreplay --live-config "$(get_configerator_relative_path "${live_config}")" --debug < "$WIREPROTO_LOGGING_PATH" 2>&1 | grep "not admitted"
  * Request was not admitted (glob)
  * Request was not admitted (glob)
  * Request was not admitted (glob)

# Check that replaying with max_concurrency = 1 replays in oder
  $ truncate -s 0 "$fastreplay_log"
  $ live_config="${LOCAL_CONFIGERATOR_PATH}/live.json"
  $ cat > "$live_config" << EOF
  > {
  >   "admission_rate": 100,
  >   "max_concurrency": 1,
  >   "scuba_sampling_target": 1,
  >   "skipped_repos": []
  > }
  > EOF
  $ quiet fastreplay  --live-config "$(get_configerator_relative_path "${live_config}")" --debug --scuba-log-file "$fastreplay_log" < "$WIREPROTO_LOGGING_PATH"
  $ grep "Replay Succeeded" "$fastreplay_log" | jq .normal.command
  "getbundle"
  "gettreepack"
  "getpackv1"

# Check that replaying works with arguments
  $ truncate -s 0 "$fastreplay_log"
  $ quiet fastreplay --debug --scuba-log-file "$fastreplay_log" -- cat "$WIREPROTO_LOGGING_PATH"
  $ grep "Replay Succeeded" "$fastreplay_log" | jq .normal.command | sort
  "getbundle"
  "getpackv1"
  "gettreepack"

# Check that replaying with skipped_repos does not replay
  $ truncate -s 0 "$fastreplay_log"
  $ live_config="${LOCAL_CONFIGERATOR_PATH}/live.json"
  $ cat > "$live_config" << EOF
  > {
  >   "admission_rate": 100,
  >   "max_concurrency": 1,
  >   "scuba_sampling_target": 1,
  >   "skipped_repos": ["repo"]
  > }
  > EOF
  $ quiet fastreplay  --live-config "$(get_configerator_relative_path "${live_config}")" --debug --scuba-log-file "$fastreplay_log" < "$WIREPROTO_LOGGING_PATH"
  $ grep "Replay Succeeded" "$fastreplay_log" | jq .normal.command
