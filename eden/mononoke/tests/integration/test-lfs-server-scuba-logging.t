# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

# Create a repository. We use MULTIPLEXED here because that is the one that records BlobGets counters.
  $ setup_common_config "blob_files"
  $ MULTIPLEXED=1 REPOID=1 FILESTORE=1 FILESTORE_CHUNK_SIZE=10 setup_mononoke_repo_config lfs1

# Start a LFS server for this repository (no upstream, but we --always-wait-for-upstream to get logging consistency)
  $ SCUBA="$TESTTMP/scuba.json"
  $ lfs_log="$TESTTMP/lfs.log"
  $ lfs_root="$(lfs_server --log "$lfs_log" --always-wait-for-upstream --scuba-dataset "file://$SCUBA")"

# Send some data
  $ yes A 2>/dev/null | head -c 2KiB | hg --config extensions.lfs= debuglfssend "${lfs_root}/lfs1"
  ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746 2048

# Read it back
  $ hg --config extensions.lfs= debuglfsreceive ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746 2048 "${lfs_root}/lfs1" | sha256sum
  ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746  -

# Finally, send an extra query to do a little more ad-hoc testing
  $ curl -fsSL -o /dev/null "${lfs_root}/config?foo=bar"

# Check that Scuba logs are present
  $ wait_for_json_record_count "$SCUBA" 5
  $ format_single_scuba_sample_strip_server_info < "$SCUBA"
  {
    "int": {
      "BlobGets": 1,
      "BlobGetsMaxLatency": *, (glob)
      "BlobGetsNotFound": 1,
      "BlobGetsNotFoundMaxLatency": *, (glob)
      "BlobGetsTotalSize": 0,
      "BlobPresenceChecks": 0,
      "BlobPresenceChecksMaxLatency": *, (glob)
      "BlobPuts": 0,
      "BlobPutsMaxLatency": *, (glob)
      "BlobPutsTotalSize": 0,
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
      "batch_context_ready_us": *, (glob)
      "batch_object_count": 1,
      "batch_request_parsed_us": *, (glob)
      "batch_request_received_us": *, (glob)
      "batch_response_ready_us": *, (glob)
      "duration_ms": *, (glob)
      "error_count": 0,
      "headers_duration_ms": *, (glob)
      "http_status": 200,
      "request_content_length": *, (glob)
      "request_load": *, (glob)
      "response_bytes_sent": *, (glob)
      "response_content_length": *, (glob)
      "seq": 0,
      "time": * (glob)
    },
    "normal": {
      "client_hostname": "localhost",
      "client_ip": "$LOCALIP",
      "http_host": "*", (glob)
      "http_method": "POST",
      "http_path": "/lfs1/objects/batch",
      "http_user_agent": "mercurial/* git/*", (glob)
      "method": "batch",
      "repository": "lfs1",
      "request_id": "*" (glob)
    },
    "normvector": {
      "client_identities": []
    }
  }
  {
    "int": {
      "BlobGets": 0,
      "BlobGetsMaxLatency": *, (glob)
      "BlobGetsNotFound": 0,
      "BlobGetsNotFoundMaxLatency": *, (glob)
      "BlobGetsTotalSize": 0,
      "BlobPresenceChecks": 0,
      "BlobPresenceChecksMaxLatency": *, (glob)
      "BlobPuts": 210,
      "BlobPutsMaxLatency": *, (glob)
      "BlobPutsTotalSize": 10930,
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
      "headers_duration_ms": *, (glob)
      "http_status": 200,
      "request_bytes_received": 2048,
      "request_content_length": 2048,
      "request_load": *, (glob)
      "response_bytes_sent": 0,
      "response_content_length": 0,
      "seq": 1,
      "time": * (glob)
    },
    "normal": {
      "client_hostname": "localhost",
      "client_ip": "$LOCALIP",
      "http_host": "*", (glob)
      "http_method": "PUT",
      "http_path": "/lfs1/upload/ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746/2048",
      "http_user_agent": "mercurial/* git/*", (glob)
      "method": "upload",
      "repository": "lfs1",
      "request_id": "*" (glob)
    },
    "normvector": {
      "client_identities": []
    }
  }
  {
    "int": {
      "BlobGets": 2,
      "BlobGetsMaxLatency": *, (glob)
      "BlobGetsNotFound": 0,
      "BlobGetsNotFoundMaxLatency": *, (glob)
      "BlobGetsTotalSize": 155,
      "BlobPresenceChecks": 0,
      "BlobPresenceChecksMaxLatency": *, (glob)
      "BlobPuts": 0,
      "BlobPutsMaxLatency": *, (glob)
      "BlobPutsTotalSize": 0,
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
      "batch_context_ready_us": *, (glob)
      "batch_object_count": 1,
      "batch_request_parsed_us": *, (glob)
      "batch_request_received_us": *, (glob)
      "batch_response_ready_us": *, (glob)
      "duration_ms": *, (glob)
      "error_count": 0,
      "headers_duration_ms": *, (glob)
      "http_status": 200,
      "request_content_length": *, (glob)
      "request_load": *, (glob)
      "response_bytes_sent": *, (glob)
      "response_content_length": *, (glob)
      "seq": 2,
      "time": * (glob)
    },
    "normal": {
      "batch_order": "*", (glob)
      "client_hostname": "localhost",
      "client_ip": "$LOCALIP",
      "http_host": "*", (glob)
      "http_method": "POST",
      "http_path": "/lfs1/objects/batch",
      "http_user_agent": "mercurial/* git/*", (glob)
      "method": "batch",
      "repository": "lfs1",
      "request_id": "*" (glob)
    },
    "normvector": {
      "batch_internal_missing_blobs": [],
      "client_identities": []
    }
  }
  {
    "int": {
      "BlobGets": 206,
      "BlobGetsMaxLatency": *, (glob)
      "BlobGetsNotFound": 0,
      "BlobGetsNotFoundMaxLatency": *, (glob)
      "BlobGetsTotalSize": 10701,
      "BlobPresenceChecks": 0,
      "BlobPresenceChecksMaxLatency": *, (glob)
      "BlobPuts": 0,
      "BlobPutsMaxLatency": *, (glob)
      "BlobPutsTotalSize": 0,
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
      "download_content_size": 2048,
      "duration_ms": *, (glob)
      "error_count": 0,
      "headers_duration_ms": *, (glob)
      "http_status": 200,
      "request_load": *, (glob)
      "response_bytes_sent": 2048,
      "response_content_length": 2048,
      "seq": 3,
      "time": * (glob)
    },
    "normal": {
      "client_hostname": "localhost",
      "client_ip": "$LOCALIP",
      "http_host": "*", (glob)
      "http_method": "GET",
      "http_path": "/lfs1/download/d28548bc21aabf04d143886d717d72375e3deecd0dafb3d110676b70a192cb5d",
      "http_user_agent": "mercurial/* git/*", (glob)
      "method": "download",
      "repository": "lfs1",
      "request_id": "*" (glob)
    },
    "normvector": {
      "client_identities": []
    }
  }
  {
    "int": {
      "BlobGets": 0,
      "BlobGetsMaxLatency": *, (glob)
      "BlobGetsNotFound": 0,
      "BlobGetsNotFoundMaxLatency": *, (glob)
      "BlobGetsTotalSize": 0,
      "BlobPresenceChecks": 0,
      "BlobPresenceChecksMaxLatency": *, (glob)
      "BlobPuts": 0,
      "BlobPutsMaxLatency": *, (glob)
      "BlobPutsTotalSize": 0,
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
      "headers_duration_ms": *, (glob)
      "http_status": 200,
      "request_load": *, (glob)
      "seq": 4,
      "time": * (glob)
    },
    "normal": {
      "client_hostname": "localhost",
      "client_ip": "$LOCALIP",
      "http_host": *, (glob)
      "http_method": "GET",
      "http_path": "/config",
      "http_query": "foo=bar",
      "http_user_agent": "curl/*", (glob)
      "request_id": * (glob)
    },
    "normvector": {
      "client_identities": []
    }
  }

# Send an invalid request and check that this gets logged
  $ truncate -s 0 "$SCUBA"
  $ curl -fsSL "${lfs_root}/lfs1/download/bad" -o /dev/null
  curl: (22) The requested URL returned error: 400 Bad Request
  [22]
  $ wait_for_json_record_count "$SCUBA" 1
  $ jq -r .normal.error_msg < "$SCUBA"
  Could not parse Content ID
  
  Caused by:
      invalid blake2 input: need exactly 64 hex digits

# Send a request after corrupting our data, and check that this gets logged
# too. Silence the error we get so that output variations in Curl don't break
# the test.
  $ truncate -s 0 "$SCUBA"
  $ find "$TESTTMP/blobstore_lfs1" -type f -name "*chunk*" | xargs rm
  $ curl -fsL "${lfs_root}/lfs1/download/d28548bc21aabf04d143886d717d72375e3deecd0dafb3d110676b70a192cb5d" -o /dev/null || false
  [1]
  $ wait_for_json_record_count "$SCUBA" 1
  $ jq -r .normal.error_msg < "$SCUBA"
  Chunk not found: ContentChunkId(Blake2(1504ec6ce051f99d41b82ec69ef7e9cde95054758b3e85ed73ef335f32f7263c))
  $ jq -r .int.error_count < "$SCUBA"
  1
