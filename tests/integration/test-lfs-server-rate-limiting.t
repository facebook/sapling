  $ . "${TEST_FIXTURES}/library.sh"

# Create a repository
  $ setup_mononoke_config
  $ REPOID=1 setup_mononoke_repo_config repo1
  $ LIVE_CONFIG="${TESTTMP}/live.json"
  $ cat > "$LIVE_CONFIG" << EOF
  > {
  >   "track_bytes_sent": true,
  >   "enable_consistent_routing": false,
  >   "throttle_limits": [
  >     {"counter": "mononoke.lfs.download.size_bytes_sent.sum.5", "limit": 10, "sleep_ms": 1000 }
  >   ],
  >   "acl_check": true,
  >   "enforce_acl_check": false
  > }
  > EOF

# Start an LFS server
  $ lfs_log="$TESTTMP/lfs.log"
  $ lfs_uri="$(lfs_server --log "$lfs_log" --live-config "file:${LIVE_CONFIG}")/repo1"

# Upload data
  $ yes A 2>/dev/null | head -c 2KiB | hg --config extensions.lfs= debuglfssend "$lfs_uri"
  ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746 2048

# Download the file. Note the blake2 is used here.
  $ curl -s -o /dev/null -w "%{http_code}\n" "${lfs_uri}/download/d28548bc21aabf04d143886d717d72375e3deecd0dafb3d110676b70a192cb5d"
  200

# Give stats aggregation time to complete
  $ sleep 2

# Next request should be throttled
  $ curl -s -o /dev/null -w "%{http_code}\n" "${lfs_uri}/download/d28548bc21aabf04d143886d717d72375e3deecd0dafb3d110676b70a192cb5d"
  429
