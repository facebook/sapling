# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup configuration
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
