# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

# Create a repository
  $ setup_mononoke_config
  $ REPOID=1 FILESTORE=1 FILESTORE_CHUNK_SIZE=10 setup_mononoke_repo_config lfs1

# Start a LFS server for this repository (no upstream)
  $ lfs_uri="$(lfs_server --tls)/health_check"

# Connecting without a client certificate fails (note: we use -k here to see the server closing the conection)
  $ curl -sk "$lfs_uri"
  [35]

# Connecting with a client certificate succeeds
  $ sslcurl -s "$lfs_uri"
  I_AM_ALIVE (no-eol)
