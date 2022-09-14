# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

# Create a repository
  $ setup_common_config
  $ REPOID=1 FILESTORE=1 FILESTORE_CHUNK_SIZE=10 setup_mononoke_repo_config repo1
  $ enable lfs
  $ LIVE_CONFIG="${LOCAL_CONFIGERATOR_PATH}/live.json"
  $ cat > "$LIVE_CONFIG" << EOF
  > {
  >   "enforce_authentication": true,
  >   "track_bytes_sent": true,
  >   "enable_consistent_routing": false,
  >   "disable_hostname_logging": false,
  >   "enforce_acl_check": true,
  >   "tasks_per_content": 1
  > }
  > EOF

# Start an LFS server for this repository
  $ SCUBA="$TESTTMP/scuba.json"
  $ LFS_LOG="$TESTTMP/lfs.log"
  $ LFS_URI="$(lfs_server --log "$LFS_LOG" --tls --scuba-log-file "$SCUBA"  --live-config "$(get_configerator_relative_path "${LIVE_CONFIG}")")"
  $ LFS_URI_REPO1="$LFS_URI/repo1"
  $ LFS_URI_HEALTH_CHECK="$LFS_URI/health_check"

# Upload a blob
  $ yes A 2>/dev/null | head -c 2KiB | hg debuglfssend "$LFS_URI_REPO1"
  ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746 2048

# Check for identities provided in X509 cert
  $ wait_for_json_record_count "$SCUBA" 2
  $ diff <(
  >   jq -S .normvector.client_identities "$SCUBA"
  > ) <(
  >   printf "$JSON_CLIENT_ID\n$JSON_CLIENT_ID" | jq -S .
  > )

# Make a request to health check endpoint from trusted proxy
# (this is a common usecase as the proxies need to check for the pool health)
# Such request won't have identities in the header as there's no downstream client.
  $ sslcurlas proxy -s "$LFS_URI_HEALTH_CHECK"
  {"message:"Client not authenticated", "request_id":"*"} (no-eol) (glob)
