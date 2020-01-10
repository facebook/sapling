  $ . "${TEST_FIXTURES}/library.sh"

# Create a repository
  $ setup_mononoke_config
  $ REPOID=1 FILESTORE=1 FILESTORE_CHUNK_SIZE=10 setup_mononoke_repo_config repo1
  $ LIVE_CONFIG="${TESTTMP}/live.json"
  $ cat > "$LIVE_CONFIG" << EOF
  > {
  >   "track_bytes_sent": true,
  >   "enable_consistent_routing": false,
  >   "throttle_limits": [],
  >   "acl_check": false,
  >   "enforce_acl_check": false
  > }
  > EOF

# Start an LFS server
  $ lfs_log="$TESTTMP/lfs.log"
  $ lfs_root="$(lfs_server --log "$lfs_log" --live-config "file:${LIVE_CONFIG}")"

# Get the config
  $ curl -fs "${lfs_root}/config" | jq -S .
  {
    "acl_check": false,
    "enable_consistent_routing": false,
    "enforce_acl_check": false,
    "throttle_limits": [],
    "track_bytes_sent": true
  }

# Update the config
  $ cat > "$LIVE_CONFIG" << EOF
  > {
  >   "track_bytes_sent": false,
  >   "enable_consistent_routing": false,
  >   "throttle_limits": [],
  >   "acl_check": false,
  >   "enforce_acl_check": false
  > }
  > EOF

# Wait for it to be updated
  $ sleep 2
  $ grep "$LIVE_CONFIG" "$lfs_log"
  * Updated path $TESTTMP/live.json (glob)

# Get the updated config
  $ curl -fs "${lfs_root}/config" | jq -S .
  {
    "acl_check": false,
    "enable_consistent_routing": false,
    "enforce_acl_check": false,
    "throttle_limits": [],
    "track_bytes_sent": false
  }
