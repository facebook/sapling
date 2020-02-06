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
  $ quiet hgmn pull

Wait for requests to be logged
  $ wait_for_json_record_count "$WIREPROTO_LOGGING_PATH" 1
  $ jq -r .normal.command "$WIREPROTO_LOGGING_PATH"
  getbundle

Replay traffic
  $ quiet traffic_replay < "$WIREPROTO_LOGGING_PATH"

Make sure more traffic was logged
  $ wait_for_json_record_count "$WIREPROTO_LOGGING_PATH" 2
