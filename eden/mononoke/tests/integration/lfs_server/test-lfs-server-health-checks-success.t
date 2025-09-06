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
  >    "bools": {
  >      "scm/mononoke:health_check_scuba_log_enabled": true
  >    },
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
  $ format_single_scuba_sample_strip_server_info < "$SCUBA"
  {
    "int": {
      "BlobGets": 0,
      "BlobGetsMaxLatency": *, (glob)
      "BlobGetsMaxSize": 0,
      "BlobGetsNotFound": 0,
      "BlobGetsNotFoundMaxLatency": *, (glob)
      "BlobGetsTotalSize": 0,
      "BlobPresenceChecks": 0,
      "BlobPresenceChecksMaxLatency": *, (glob)
      "BlobPuts": 0,
      "BlobPutsMaxLatency": *, (glob)
      "BlobPutsMaxSize": 0,
      "BlobPutsTotalSize": 0,
      "BlobUnlinks": 0,
      "BlobUnlinksMaxLatency": *, (glob)
      "CachelibHits": 0,
      "CachelibMisses": 0,
      "GetpackNumPossibleLFSFiles": 0,
      "GetpackPossibleLFSFilesSumSize": 0,
      "GettreepackDesignatedNodes": 0,
      "MemcacheHits": 0,
      "MemcacheMisses": 0,
      "SqlReadsMaster": 0,
      "SqlReadsReplica": 0,
      "SqlWrites": 0,
      "duration_ms": *, (glob)
      "error_count": 0,
      "headers_duration_ms": 0,
      "http_status": 200,
      "sample_rate": 1,
      "seq": 0,
      "time": * (glob)
    },
    "normal": {
      "client_correlator": *, (glob)
      "client_entry_point": "curl_test",
      "client_hostname": "localhost",
      "client_ip": "$LOCALIP",
      "client_main_id": *, (glob)
      "fetch_cause": null,
      "fetch_from_cas_attempted": "false",
      "http_host": *, (glob)
      "http_method": "GET",
      "http_path": "/health_check",
      "http_user_agent": "curl/*", (glob)
      "read_bookmarks_from_xdb_replica": "true",
      "request_id": "*", (glob)
      "sandcastle_alias": null,
      "sandcastle_nonce": null,
      "sandcastle_vcs": null
    },
    "normvector": {
      "client_identities": [],
      "use_maybe_stale_freshness_for_bookmarks": [
        "mononoke_api::repo::git::get_bookmark_state"
      ]
    }
  }
