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
  >   "acl_check": true,
  >   "enforce_acl_check": false
  > }
  > EOF

# Start an LFS server
  $ LFS_LOG="$TESTTMP/lfs.log"
  $ LFS_ROOT="$(lfs_server --log "$LFS_LOG" --tls --live-config "file:${LIVE_CONFIG}" --allowed-test-identity USER:test --trusted-proxy-identity USER:myusername0)"
  $ LFS_URI="$LFS_ROOT/repo1"

# Setup constants. These headers are normally provided by proxygen, they store
# an encoded form of the original client identity. In this case, we have
# USER:test and USER:invalid
  $ ALLOWED_IDENT="x-fb-validated-client-encoded-identity: %7B%22ai%22%3A%20%22%22%2C%20%22ch%22%3A%20%22%22%2C%20%22it%22%3A%20%22user%22%2C%20%22id%22%3A%20%22test%22%7D"
  $ DISALLOWED_IDENT="x-fb-validated-client-encoded-identity: %7B%22ai%22%3A%20%22%22%2C%20%22ch%22%3A%20%22%22%2C%20%22it%22%3A%20%22user%22%2C%20%22id%22%3A%20%22invalid%22%7D"
  $ DOWNLOAD_URL="$LFS_URI/download/d28548bc21aabf04d143886d717d72375e3deecd0dafb3d110676b70a192cb5d"

# Upload a blob
  $ yes A 2>/dev/null | head -c 2KiB | ssldebuglfssend "$LFS_URI"
  ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746 2048

# Enable ACL checking
  $ sed -i 's/"enforce_acl_check": false/"enforce_acl_check": true/g' "$LIVE_CONFIG"
  $ sleep 2

# Make a request with a valid encoded client identity header
# NOTE: The LFS Server trusts the identity sslcurl passes as a trusted proxy
  $ sslcurl -s -o /dev/null -w "%{http_code}\n" "$DOWNLOAD_URL" --header "$ALLOWED_IDENT"
  200

# Make a request with an invalid encoded client identity header
  $ sslcurl -s -o /dev/null -w "%{http_code}\n" "$DOWNLOAD_URL" --header "$DISALLOWED_IDENT"
  403

# Make a request without specifying an identity in the header
# NOTE: We allow this whilst we wait for all clients to get certs
  $ sslcurl -s -o /dev/null -w "%{http_code}\n" "$DOWNLOAD_URL"
  200

# Disable ACL checking
  $ sed -i 's/"enforce_acl_check": true/"enforce_acl_check": false/g' "$LIVE_CONFIG"
  $ sleep 2

# Make a request with a valid encoded client identity header, but acl checking
# disabled
  $ sslcurl -s -o /dev/null -w "%{http_code}\n" "$DOWNLOAD_URL" --header "$ALLOWED_IDENT"
  200

# Make a request with an invalid encoded client identity header, but acl checking
# disabled
  $ sslcurl -s -o /dev/null -w "%{http_code}\n" "$DOWNLOAD_URL" --header "$DISALLOWED_IDENT"
  200

# Make a request without an identity in the header, but an identity provided by
# the cert curl uses
  $ sslcurl -s -o /dev/null -w "%{http_code}\n" "$DOWNLOAD_URL"
  200
