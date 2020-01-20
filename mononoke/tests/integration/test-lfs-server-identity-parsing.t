# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

# Create a repository
  $ setup_mononoke_config
  $ REPOID=1 FILESTORE=1 FILESTORE_CHUNK_SIZE=10 setup_mononoke_repo_config repo1

# Start an LFS server for this repository
  $ SCUBA="$TESTTMP/scuba.json"
  $ LFS_LOG="$TESTTMP/lfs.log"
  $ LFS_URI="$(
  > lfs_server --log "$LFS_LOG" --tls --scuba-log-file \
  > "$SCUBA")/repo1"

# Upload a blob
  $ yes A 2>/dev/null | head -c 2KiB | ssldebuglfssend "$LFS_URI"
  ab02c2a1923c8eb11cb3ddab70320746d71d32ad63f255698dc67c3295757746 2048

# Check for identities provided in X509 cert
  $ wait_for_json_record_count "$SCUBA" 2
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
