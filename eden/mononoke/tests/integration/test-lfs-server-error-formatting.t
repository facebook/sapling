# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

# Create a repository
  $ setup_common_config
  $ REPOID=1 setup_mononoke_repo_config repo1

# Start an LFS server for this repository

  $ LFS_URI="$(lfs_server)/repo1"

# Query it and cause an error

  $ curl --silent "$LFS_URI/download/foo" | jq -S .
  {
    "message": "Could not parse Content ID: invalid blake2 input: need exactly 64 hex digits",
    "request_id": "*" (glob)
  }
  $ curl --silent "$LFS_URI/download/1111111111111111111111111111111111111111111111111111111111111111" | jq -S .
  {
    "message": "Object does not exist: Canonical(ContentId(Blake2(1111111111111111111111111111111111111111111111111111111111111111)))",
    "request_id": "*" (glob)
  }
