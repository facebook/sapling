  $ . "${TEST_FIXTURES}/library.sh"

# Create a repository
  $ setup_mononoke_config
  $ FILESTORE=1 FILESTORE_CHUNK_SIZE=10 setup_mononoke_repo_config lfs1

# Start a LFS server for this repository (no upstream)
  $ lfs_uri="$(lfs_server --tls)/health_check"

# Connecting without a client certificate fails (note: we use -k here to see the server closing the conection)
  $ curl -sk "$lfs_uri"
  [35]

# Connecting with a client certificate succeeds
  $ sslcurl -s "$lfs_uri"
  I_AM_ALIVE (no-eol)
