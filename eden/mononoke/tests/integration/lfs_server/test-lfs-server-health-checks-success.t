# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

# Create a repository. We use MULTIPLEXED here because that is the one that records BlobGets counters.
  $ setup_common_config "blob_files"
  $ MULTIPLEXED=1 REPOID=1 FILESTORE=1 FILESTORE_CHUNK_SIZE=10 setup_mononoke_repo_config lfs1
  $ enable lfs

# Start a LFS server for this repository (no upstream, but we --always-wait-for-upstream to get logging consistency)
# Enable authentication as that is the easiest way to make health check fail.
# Disable local caching as we want to check each access.
# Make sure success logs are not sampled.
  $ SCUBA="$TESTTMP/scuba-hc-success.json"
  $ lfs_log="$TESTTMP/lfs.log"
  $ merge_just_knobs <<EOF
  > {
  >    "ints": {
  >      "scm/mononoke:health_check_scuba_log_failure_sampling_rate": 1,
  >      "scm/mononoke:health_check_scuba_log_success_sampling_rate": 1
  >    }
  > }
  > EOF
  $ lfs_root="$(CACHE_ARGS=--cache-mode=disabled lfs_server --log "$lfs_log" --always-wait-for-upstream --scuba-log-file "$SCUBA")"

# `lfs_server` sends a health check request (via `lfs_health`), which will emit
# a (sampled) log entry. In most cases that's ok, and logs are sample anyway.
# This test requires unsampled Scuba logs and cares about each one of them,
# so we have to consume.
  $ wait_for_json_record_count "$SCUBA" 1

# Send a health check request
  $ truncate -s 0 "$SCUBA"
  $ curltest -fsSL "${lfs_root}/health_check"
  I_AM_ALIVE (no-eol)
  $ wait_for_json_record_count "$SCUBA" 1
  $ jq -S '{http_status: .int.http_status, http_method: .normal.http_method, http_path: .normal.http_path}' < "$SCUBA"
  {
    "http_method": "GET",
    "http_path": "/health_check",
    "http_status": 200
  }
