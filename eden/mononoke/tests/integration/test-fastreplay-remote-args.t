# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ export WIREPROTO_LOGGING_PATH="$TESTTMP/wireproto.json"
  $ export WIREPROTO_LOGGING_BLOBSTORE="$TESTTMP/traffic-replay-blobstore"
  $ BLOB_TYPE="blob_files" quiet default_setup

Make requests
  $ quiet hgmn  pull
  $ quiet hgmn up master_bookmark

Wait for requests to be logged
  $ wait_for_json_record_count "$WIREPROTO_LOGGING_PATH" 3
  $ jq -r .normal.command "$WIREPROTO_LOGGING_PATH" | sort
  getbundle
  getpackv1
  gettreepack

Replay traffic using the ephemeral blobstore
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

Delete the ephemeral blobstore data. Check that replay now fails.
  $ rm -r "$WIREPROTO_LOGGING_BLOBSTORE"
  $ fastreplay < "$WIREPROTO_LOGGING_PATH" 2>&1 | grep -A6 "Dispatch failed"
  * Dispatch failed: Error { (glob)
      context: "While parsing request",
      source: Error {
          context: "While loading remote_args",
          source: "Key not found: wireproto_replay.*", (glob)
      },
  }
  * Dispatch failed: Error { (glob)
      context: "While parsing request",
      source: Error {
          context: "While loading remote_args",
          source: "Key not found: wireproto_replay.*", (glob)
      },
  }
  * Dispatch failed: Error { (glob)
      context: "While parsing request",
      source: Error {
          context: "While loading remote_args",
          source: "Key not found: wireproto_replay.*", (glob)
      },
  }
