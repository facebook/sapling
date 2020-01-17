  $ . "${TEST_FIXTURES}/library.sh"

# Create a repository
  $ setup_mononoke_config
  $ MULTIPLEXED=1 REPOID=1 FILESTORE=1 FILESTORE_CHUNK_SIZE=10 setup_mononoke_repo_config lfs1
  $ LIVE_CONFIG="${TESTTMP}/live.json"
  $ cat > "$LIVE_CONFIG" << EOF
  > {
  >   "track_bytes_sent": true,
  >   "enable_consistent_routing": false,
  >   "disable_hostname_logging": true,
  >   "throttle_limits": [],
  >   "acl_check": false,
  >   "enforce_acl_check": false
  > }
  > EOF

# Start an LFS server for this repository
  $ SCUBA="$TESTTMP/scuba.json"
  $ lfs_log="$TESTTMP/lfs.log"
  $ lfs_root="$(lfs_server --log "$lfs_log" --scuba-log-file "$SCUBA" --live-config "file:${LIVE_CONFIG}")"

# Get the config
  $ curl -fs "${lfs_root}/config" | jq -S .
  {
    "acl_check": false,
    "disable_hostname_logging": true,
    "enable_consistent_routing": false,
    "enforce_acl_check": false,
    "throttle_limits": [],
    "track_bytes_sent": true
  }

# Send some data
  $ yes A 2>/dev/null | head -c 2KiB | hg --config extensions.lfs= debuglfssend "${lfs_root}/lfs1"
  ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746 2048

# Check that Scuba logs *do not* contain `client_hostname`
  $ wait_for_json_record_count "$SCUBA" 3
  $ jq -S .normal.client_hostname < "$SCUBA"
  null
  null
  null

# Update the config
  $ cat > "$LIVE_CONFIG" << EOF
  > {
  >   "track_bytes_sent": false,
  >   "enable_consistent_routing": false,
  >   "disable_hostname_logging": false,
  >   "throttle_limits": [],
  >   "acl_check": false,
  >   "enforce_acl_check": false
  > }
  > EOF

# Wait for the config to be updated
  $ sleep 2

# Send some data
  $ yes A 2>/dev/null | head -c 2KiB | hg --config extensions.lfs= debuglfssend "${lfs_root}/lfs1"
  ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746 2048

# Check that Scuba logs contain `client_hostname`
  $ wait_for_json_record_count "$SCUBA" 4
  $ jq -S .normal.client_hostname < "$SCUBA"
  null
  null
  null
  "localhost"
