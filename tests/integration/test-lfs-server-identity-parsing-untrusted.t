  $ . "${TEST_FIXTURES}/library.sh"

# Create a repository
  $ setup_mononoke_config
  $ REPOID=1 FILESTORE=1 FILESTORE_CHUNK_SIZE=10 setup_mononoke_repo_config repo1

# Start an LFS server for this repository
  $ SCUBA="$TESTTMP/scuba.json"
  $ LFS_LOG="$TESTTMP/lfs.log"
  $ LFS_URI="$(
  > lfs_server --log "$LFS_LOG" --tls --scuba-log-file "$SCUBA" --trusted-proxy-identity USER:foo123)/repo1"

# Setup constants
  $ ALLOWED_IDENT="x-fb-validated-client-encoded-identity: %7B%22ai%22%3A%20%22%22%2C%20%22ch%22%3A%20%22%22%2C%20%22it%22%3A%20%22user%22%2C%20%22id%22%3A%20%22test%22%7D"
  $ DOWNLOAD_URL="$LFS_URI/download/d28548bc21aabf04d143886d717d72375e3deecd0dafb3d110676b70a192cb5d"

# Upload a blob
  $ truncate -s 0 "$SCUBA"
  $ yes A 2>/dev/null | head -c 2KiB | ssldebuglfssend "$LFS_URI"
  ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746 2048

# Check for identities from header
  $ wait_for_nonempty_file "$SCUBA"
  $ jq -S .normvector.client_identities < "$SCUBA"
  [
    "USER:myusername0",
    "MACHINE:devvm000.lla0.facebook.com",
    "MACHINE_TIER:devvm"
  ]
  [
    "USER:myusername0",
    "MACHINE:devvm000.lla0.facebook.com",
    "MACHINE_TIER:devvm"
  ]

# Make a request with a valid encoded client identity header, but without being
# the trusted proxy ident. This means that the LFS server should parse our
# client idents from the cert we provide.
  $ truncate -s 0 "$SCUBA"
  $ sslcurl -s -o /dev/null -w "%{http_code}\n" "$DOWNLOAD_URL" --header "$ALLOWED_IDENT"
  200

  $ wait_for_nonempty_file "$SCUBA"
  $ jq -S .normvector.client_identities < "$SCUBA"
  [
    "USER:myusername0",
    "MACHINE:devvm000.lla0.facebook.com",
    "MACHINE_TIER:devvm"
  ]
