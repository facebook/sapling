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
  $ lfs_log="$TESTTMP/lfs.log"
  $ lfs_uri="$(lfs_server --log "$lfs_log" --max-upload-size 10)/lfs1"

# Send an acceptable file
  $ yes A 2>/dev/null | head -c 10 | hg --config extensions.lfs= debuglfssend "$lfs_uri"
  * 10 (glob)

# Send an unacceptable file
  $ yes A 2>/dev/null | head -c 11 | hg --config extensions.lfs= debuglfssend "$lfs_uri"
  abort: LFS server error: *Object size (11) exceeds max allowed size (10)* (glob)
  [255]

# Verify that direct uploads fail too
  $ curl -s --upload-file /dev/null "${lfs_uri}/upload/1111111111111111111111111111111111111111111111111111111111111111/11"
  {"message":"Object size (11) exceeds max allowed size (10)","request_id":"*"} (no-eol) (glob)
