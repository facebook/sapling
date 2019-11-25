  $ . "${TEST_FIXTURES}/library.sh"

# Create a repository. We use MULTIPLEXED here because that is the one that records BlobGets counters.
  $ setup_mononoke_config
  $ MULTIPLEXED=1 REPOID=1 FILESTORE=1 FILESTORE_CHUNK_SIZE=10 setup_mononoke_repo_config lfs1

# Start a LFS server for this repository (no upstream, but we --always-wait-for-upstream to get logging consistency)
  $ SCUBA="$TESTTMP/scuba.json"
  $ lfs_log="$TESTTMP/lfs.log"
  $ lfs_root="$(
  > SMC_TIERS=foo.bar \
  > TW_TASK_ID=123 \
  > TW_CANARY_ID=canary \
  > TW_JOB_CLUSTER=foo \
  > TW_JOB_USER=bar \
  > TW_JOB_NAME=qux \
  > lfs_server --log "$lfs_log" --always-wait-for-upstream --scuba-log-file "$SCUBA")"

# Send some data
  $ yes A 2>/dev/null | head -c 2KiB | hg --config extensions.lfs= debuglfssend "${lfs_root}/lfs1"
  ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746 2048

# Read it back
  $ hg --config extensions.lfs= debuglfsreceive ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746 2048 "${lfs_root}/lfs1" | sha256sum
  ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746  -

# Finally, send an extra query to do a little more ad-hoc testing
  $ curl -fsSL -o /dev/null "${lfs_root}/config?foo=bar"

# Check that Scuba logs are present
  $ jq -S . < "$SCUBA"
  {
    "int": {
      "BlobGets": 1,
      "BlobGetsMaxLatency": *, (glob)
      "BlobPresenceChecks": 0,
      "BlobPresenceChecksMaxLatency": *, (glob)
      "BlobPuts": 0,
      "BlobPutsMaxLatency": *, (glob)
      "CachelibHits": 0,
      "CachelibMisses": 0,
      "MemcacheHits": 0,
      "MemcacheMisses": 0,
      "SqlReadsMaster": 0,
      "SqlReadsReplica": 0,
      "SqlWrites": 0,
      "batch_object_count": 1,
      "duration_ms": *, (glob)
      "headers_duration_ms": *, (glob)
      "http_status": 200,
      "request_content_length": *, (glob)
      "request_load": *, (glob)
      "response_bytes_sent": *, (glob)
      "response_content_length": *, (glob)
      "time": * (glob)
    },
    "normal": {
      "build_revision": *, (glob)
      "client_hostname": "localhost",
      "client_ip": "$LOCALIP",
      "http_host": "*", (glob)
      "http_method": "POST",
      "http_path": "/lfs1/objects/batch",
      "method": "batch",
      "repository": "lfs1",
      "request_id": "*", (glob)
      "server_hostname": "*", (glob)
      "server_tier": "foo.bar",
      "tw_canary_id": "canary",
      "tw_handle": "foo/bar/qux",
      "tw_task_id": "123"
    }
  }
  {
    "int": {
      "BlobGets": 0,
      "BlobGetsMaxLatency": *, (glob)
      "BlobPresenceChecks": 0,
      "BlobPresenceChecksMaxLatency": *, (glob)
      "BlobPuts": 210,
      "BlobPutsMaxLatency": *, (glob)
      "CachelibHits": 0,
      "CachelibMisses": 0,
      "MemcacheHits": 0,
      "MemcacheMisses": 0,
      "SqlReadsMaster": 0,
      "SqlReadsReplica": 0,
      "SqlWrites": 0,
      "duration_ms": *, (glob)
      "headers_duration_ms": *, (glob)
      "http_status": 200,
      "request_bytes_received": 2048,
      "request_content_length": 2048,
      "request_load": *, (glob)
      "response_bytes_sent": 0,
      "response_content_length": 0,
      "time": * (glob)
    },
    "normal": {
      "build_revision": *, (glob)
      "client_hostname": "localhost",
      "client_ip": "$LOCALIP",
      "http_host": "*", (glob)
      "http_method": "PUT",
      "http_path": "/lfs1/upload/ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746/2048",
      "method": "upload",
      "repository": "lfs1",
      "request_id": "*", (glob)
      "server_hostname": "*", (glob)
      "server_tier": "foo.bar",
      "tw_canary_id": "canary",
      "tw_handle": "foo/bar/qux",
      "tw_task_id": "123"
    }
  }
  {
    "int": {
      "BlobGets": 2,
      "BlobGetsMaxLatency": *, (glob)
      "BlobPresenceChecks": 0,
      "BlobPresenceChecksMaxLatency": *, (glob)
      "BlobPuts": 0,
      "BlobPutsMaxLatency": *, (glob)
      "CachelibHits": 0,
      "CachelibMisses": 0,
      "MemcacheHits": 0,
      "MemcacheMisses": 0,
      "SqlReadsMaster": 0,
      "SqlReadsReplica": 0,
      "SqlWrites": 0,
      "batch_object_count": 1,
      "duration_ms": *, (glob)
      "headers_duration_ms": *, (glob)
      "http_status": 200,
      "request_content_length": *, (glob)
      "request_load": *, (glob)
      "response_bytes_sent": *, (glob)
      "response_content_length": *, (glob)
      "time": * (glob)
    },
    "normal": {
      "batch_order": "*", (glob)
      "build_revision": *, (glob)
      "client_hostname": "localhost",
      "client_ip": "$LOCALIP",
      "http_host": "*", (glob)
      "http_method": "POST",
      "http_path": "/lfs1/objects/batch",
      "method": "batch",
      "repository": "lfs1",
      "request_id": "*", (glob)
      "server_hostname": "*", (glob)
      "server_tier": "foo.bar",
      "tw_canary_id": "canary",
      "tw_handle": "foo/bar/qux",
      "tw_task_id": "123"
    },
    "normvector": {
      "batch_internal_missing_blobs": []
    }
  }
  {
    "int": {
      "BlobGets": 206,
      "BlobGetsMaxLatency": *, (glob)
      "BlobPresenceChecks": 0,
      "BlobPresenceChecksMaxLatency": *, (glob)
      "BlobPuts": 0,
      "BlobPutsMaxLatency": *, (glob)
      "CachelibHits": 0,
      "CachelibMisses": 0,
      "MemcacheHits": 0,
      "MemcacheMisses": 0,
      "SqlReadsMaster": 0,
      "SqlReadsReplica": 0,
      "SqlWrites": 0,
      "duration_ms": *, (glob)
      "headers_duration_ms": *, (glob)
      "http_status": 200,
      "request_load": *, (glob)
      "response_bytes_sent": 2048,
      "response_content_length": 2048,
      "time": * (glob)
    },
    "normal": {
      "build_revision": *, (glob)
      "client_hostname": "localhost",
      "client_ip": "$LOCALIP",
      "http_host": "*", (glob)
      "http_method": "GET",
      "http_path": "/lfs1/download/d28548bc21aabf04d143886d717d72375e3deecd0dafb3d110676b70a192cb5d",
      "method": "download",
      "repository": "lfs1",
      "request_id": "*", (glob)
      "server_hostname": "*", (glob)
      "server_tier": "foo.bar",
      "tw_canary_id": "canary",
      "tw_handle": "foo/bar/qux",
      "tw_task_id": "123"
    }
  }
  {
    "int": {
      "BlobGets": 0,
      "BlobGetsMaxLatency": *, (glob)
      "BlobPresenceChecks": 0,
      "BlobPresenceChecksMaxLatency": *, (glob)
      "BlobPuts": 0,
      "BlobPutsMaxLatency": *, (glob)
      "CachelibHits": 0,
      "CachelibMisses": 0,
      "MemcacheHits": 0,
      "MemcacheMisses": 0,
      "SqlReadsMaster": 0,
      "SqlReadsReplica": 0,
      "SqlWrites": 0,
      "duration_ms": *, (glob)
      "headers_duration_ms": *, (glob)
      "http_status": 200,
      "request_load": *, (glob)
      "response_bytes_sent": *, (glob)
      "response_content_length": *, (glob)
      "time": * (glob)
    },
    "normal": {
      "build_revision": *, (glob)
      "client_hostname": "localhost",
      "client_ip": "$LOCALIP",
      "http_host": *, (glob)
      "http_method": "GET",
      "http_path": "/config",
      "http_query": "foo=bar",
      "request_id": *, (glob)
      "server_hostname": *, (glob)
      "server_tier": "foo.bar",
      "tw_canary_id": "canary",
      "tw_handle": "foo/bar/qux",
      "tw_task_id": "123"
    }
  }
