# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ setup_common_config
  $ enable lfs

# Create a repository without ACL checking enforcement
  $ REPOID=1 FILESTORE=1 FILESTORE_CHUNK_SIZE=10 setup_mononoke_repo_config repo1

# Create a repository with ACL checking enforcement
  $ ENFORCE_LFS_ACL_CHECK=1 REPOID=2 FILESTORE=1 FILESTORE_CHUNK_SIZE=10 setup_mononoke_repo_config repo2

  $ LIVE_CONFIG="${LOCAL_CONFIGERATOR_PATH}/live.json"
  $ cat > "$LIVE_CONFIG" << EOF
  > {
  >   "track_bytes_sent": true,
  >   "enable_consistent_routing": false,
  >   "disable_hostname_logging": false,
  >   "enforce_acl_check": false,
  >   "tasks_per_content": 1
  > }
  > EOF

# Start an LFS server
  $ LFS_LOG="$TESTTMP/lfs.log"
  $ LFS_ROOT="$(lfs_server --log "$LFS_LOG" --tls --live-config "$(get_configerator_relative_path "${LIVE_CONFIG}")" --allowed-test-identity USER:test --trusted-proxy-identity SERVICE_IDENTITY:proxy)"
  $ LFS_URI="$LFS_ROOT/repo1"
  $ LFS_URI_REPO_ENFORCE_ACL="$LFS_ROOT/repo2"

# Setup constants. These headers are normally provided by proxygen, they store
# an encoded form of the original client identity. In this case, we have
# USER:test and USER:invalid
  $ ALLOWED_IDENT="x-fb-validated-client-encoded-identity: %7B%22ai%22%3A%20%22%22%2C%20%22ch%22%3A%20%22%22%2C%20%22it%22%3A%20%22user%22%2C%20%22id%22%3A%20%22test%22%7D"
  $ DISALLOWED_IDENT="x-fb-validated-client-encoded-identity: %7B%22ai%22%3A%20%22%22%2C%20%22ch%22%3A%20%22%22%2C%20%22it%22%3A%20%22user%22%2C%20%22id%22%3A%20%22invalid%22%7D"
  $ DOWNLOAD_URL="$LFS_URI/download/d28548bc21aabf04d143886d717d72375e3deecd0dafb3d110676b70a192cb5d"
  $ DOWNLOAD_URL_REPO_ENFORCE_ACL="$LFS_URI_REPO_ENFORCE_ACL/download/d28548bc21aabf04d143886d717d72375e3deecd0dafb3d110676b70a192cb5d"

# Upload a blob to both repos
  $ yes A 2>/dev/null | head -c 2KiB | hg debuglfssend "$LFS_URI_REPO_ENFORCE_ACL"
  ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746 2048

  $ yes A 2>/dev/null | head -c 2KiB | hg debuglfssend "$LFS_URI"
  ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746 2048

# Make a request with a valid encoded client identity header, but acl
# enforcement disabled at both the global and repo level.
  $ sslcurl -s -o /dev/null -w "%{http_code}\n" "$DOWNLOAD_URL" --header "$ALLOWED_IDENT"
  200

# Enable ACL enforcement killswitch
  $ sed -i 's/"enforce_acl_check": false/"enforce_acl_check": true/g' "$LIVE_CONFIG"
  $ sleep 2

# Make a request with a valid encoded client identity header, but acl
# enforcement enabled at the global but not repo level.
  $ sslcurl -s -o /dev/null -w "%{http_code}\n" "$DOWNLOAD_URL" --header "$ALLOWED_IDENT"
  200

# Make a request with a valid encoded client identity header
# NOTE: The LFS Server trusts the identity sslcurl passes as a trusted proxy
  $ if [ -n "$HAS_FB" ]; then
  > diff <(
  >   sslcurl -s -o /dev/null -w "%{http_code}\n" "$DOWNLOAD_URL_REPO_ENFORCE_ACL" --header "$ALLOWED_IDENT"
  > ) <( echo "200" )
  > fi

# Make a request without specifying an identity in the header
  $ sslcurl -s -o /dev/null -w "%{http_code}\n" "$DOWNLOAD_URL_REPO_ENFORCE_ACL"
  403

# Disable ACL enforcement killswitch
  $ sed -i 's/"enforce_acl_check": true/"enforce_acl_check": false/g' "$LIVE_CONFIG"
  $ sleep 2

# Make a request with a valid encoded client identity header, but acl
# enforcement disabled
  $ sslcurl -s -o /dev/null -w "%{http_code}\n" "$DOWNLOAD_URL_REPO_ENFORCE_ACL" --header "$ALLOWED_IDENT"
  200

# Make a request with an invalid encoded client identity header, but acl
# enforcement disabled
  $ sslcurl -s -o /dev/null -w "%{http_code}\n" "$DOWNLOAD_URL_REPO_ENFORCE_ACL" --header "$DISALLOWED_IDENT"
  200

# Make a request without an identity in the header, but an identity provided by
# the cert curl uses
  $ sslcurl -s -o /dev/null -w "%{http_code}\n" "$DOWNLOAD_URL_REPO_ENFORCE_ACL"
  200
