# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ MULTIPLEXED=2 setup_common_config "blob_files"
  $ cd "$TESTTMP"

Create commits using testtool drawdag
  $ testtool_drawdag -R repo --print-hg-hashes <<'EOF'
  > A-B-C
  > # bookmark: C master_bookmark
  > EOF
  A=* (glob)
  B=* (glob)
  C=* (glob)

Base case, check the stores have expected counts
  $ ls blobstore/0/blobs/ | wc -l
  33
  $ ls blobstore/1/blobs/ | wc -l
  33
  $ ls blobstore/2/blobs/ | wc -l
  33

Populate WAL queue by simulating failed writes to blobstore 0
  $ mononoke_testtool populate-wal -R repo --blobstore-path "$TESTTMP/blobstore" --source-blobstore-id 1 --target-blobstore-id 1 --delete-target-blobs
  Found 33 blobs in source blobstore 1
  Deleted 33 blobs from target blobstore 0
  Inserted 33 WAL entries for target multiplex_id 1

Check the stores have expected counts
  $ ls blobstore/0/blobs/ | wc -l
  0
  $ ls blobstore/1/blobs/ | wc -l
  33
  $ ls blobstore/2/blobs/ | wc -l
  33

Check that healer queue has items
  $ read_blobstore_wal_queue_size
  33
