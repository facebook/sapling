# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

# Create a repository
  $ setup_common_config
  $ REPOID=1 FILESTORE=1 FILESTORE_CHUNK_SIZE=10 setup_mononoke_repo_config lfs1

# Start a LFS server for this repository (no upstream)
  $ lfs_uri="$(lfs_server --tls)/health_check"

# Connecting without a client certificate fails
#
# Note: We use -k here to see the server closing the conection. Depending on the
# version of curl (and in particular, the version TLS it uses), the return code
# may be either 35 (CURLE_SSL_CONNECT_ERROR) or 56 (CURLE_RECV_ERROR).
  $ curl -sk "$lfs_uri" || echo "$?"
  (35|56) (re)

# Connecting with a client certificate succeeds
  $ sslcurl -s "$lfs_uri"
  I_AM_ALIVE (no-eol)
