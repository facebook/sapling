# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

# Create a repository
  $ setup_common_config
  $ REPOID=1 FILESTORE=1 FILESTORE_CHUNK_SIZE=10 setup_mononoke_repo_config repo1
  $ LIVE_CONFIG="${LOCAL_CONFIGERATOR_PATH}/live.json"
  $ cat > "$LIVE_CONFIG" << EOF
  > {
  >   "track_bytes_sent": true,
  >   "enable_consistent_routing": false,
  >   "disable_hostname_logging": false,
  >   "enforce_acl_check": false
  > }
  > EOF

# Start an LFS server
  $ lfs_log="$TESTTMP/lfs.log"
  $ lfs_root="$(lfs_server --log "$lfs_log" --live-config "$(get_configerator_relative_path "${LIVE_CONFIG}")")"

# Get the config
  $ curltest -fs "${lfs_root}/config" | jq -S .
  {
    "disable_compression": false,
    "disable_compression_identities": [],
    "disable_hostname_logging": false,
    "enable_consistent_routing": false,
    "enforce_acl_check": false,
    "enforce_authentication": false,
    "loadshedding_limits": [],
    "object_popularity": null,
    "track_bytes_sent": true
  }

# Update the config
  $ cat > "$LIVE_CONFIG" << EOF
  > {
  >   "track_bytes_sent": false,
  >   "enable_consistent_routing": false,
  >   "disable_hostname_logging": false,
  >   "enforce_acl_check": false
  > }
  > EOF

# Wait for it to be updated
  $ sleep 2
  $ grep "live.json" "$lfs_log"
  * Updated path live.json (glob)

# Get the updated config
  $ curltest -fs "${lfs_root}/config" | jq -S .
  {
    "disable_compression": false,
    "disable_compression_identities": [],
    "disable_hostname_logging": false,
    "enable_consistent_routing": false,
    "enforce_acl_check": false,
    "enforce_authentication": false,
    "loadshedding_limits": [],
    "object_popularity": null,
    "track_bytes_sent": false
  }
