  $ . "${TEST_FIXTURES}/library.sh"

# Create a repository
  $ setup_mononoke_config
  $ REPOID=1 FILESTORE=1 FILESTORE_CHUNK_SIZE=10 setup_mononoke_repo_config repo1
  $ LIVE_CONFIG="${TESTTMP}/live.json"
  $ cat > "$LIVE_CONFIG" << EOF
  > {
  >   "track_bytes_sent": true,
  >   "enable_consistent_routing": false,
  >   "max_bytes_sent_5s": null,
  >   "max_bytes_sent_15s": null
  > }
  > EOF

# Start a LFS server
  $ lfs_log="$TESTTMP/lfs.log"
  $ lfs_root="$(lfs_server --log "$lfs_log" --live-config "file:${LIVE_CONFIG}")"

# Get the config
  $ curl -fs "${lfs_root}/config"
  {"track_bytes_sent":true,"enable_consistent_routing":false,"max_bytes_sent_5s":null,"max_bytes_sent_15s":null} (no-eol)

# Update the config
  $ cat > "$LIVE_CONFIG" << EOF
  > {
  >   "track_bytes_sent": false,
  >   "enable_consistent_routing": false,
  >   "max_bytes_sent_5s": null,
  >   "max_bytes_sent_15s": null
  > }
  > EOF

# Wait for it to be updated
  $ sleep 10

# Get the updated config
  $ curl -fs "${lfs_root}/config"
  {"track_bytes_sent":false,"enable_consistent_routing":false,"max_bytes_sent_5s":null,"max_bytes_sent_15s":null} (no-eol)
