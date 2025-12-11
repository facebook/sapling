# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration in usual uncompressed way
  $ MULTIPLEXED=1 PACK_BLOB=0 setup_common_config "blob_files"
  $ cd "$TESTTMP"

Create commits using testtool_drawdag to populate Mononoke
  $ testtool_drawdag -R repo --print-hg-hashes <<EOF
  > A-B-C
  > # bookmark: C master_bookmark
  > EOF
  A=* (glob)
  B=* (glob)
  C=* (glob)

Also create a local hg repo for the second blobimport with zstd
  $ hginit_treemanifest repo
  $ cd repo
  $ drawdag <<EOF
  > C
  > |
  > B
  > |
  > A
  > EOF
  $ hg bookmark master_bookmark -r $C
  $ cd "$TESTTMP"

Start server
  $ start_and_wait_for_mononoke_server

Check the stores have expected counts
  $ ls blobstore/0/blobs/ | wc -l
  33
  $ ls blobstore/1/blobs/ | wc -l
  33

Check that the packed sizes are larger due to the packblob wrappers on store 0
  $ PACKED=$(du -s --bytes blobstore/0/blobs/ | cut -f1); UNPACKED=$(du -s --bytes blobstore/1/blobs/ | cut -f1)
  $ if [[ "$PACKED" -le "$UNPACKED" ]]; then echo "expected packed $PACKED to be larger than unpacked $UNPACKED due to thift wrappers"; fi

Move the uncompressed packed store aside
  $ mv "$TESTTMP/blobstore/0" "$TESTTMP/blobstore.raw"
  $ rm -rf "$TESTTMP/blobstore_sync_queue/sqlite_dbs" "$TESTTMP/blobstore"

Blobimport again, but this time enable zstd compression
  $ blobimport repo/.hg repo --blobstore-write-zstd=true --blobstore-write-zstd-level=0

Check that the packed sizes are smaller due to compression
  $ PACKED=$(du -s --bytes blobstore/0/blobs/ | cut -f1); OLDPACKED=$(du -s --bytes blobstore.raw/blobs/ | cut -f1)
  $ if [[ "$PACKED" -ge "$OLDPACKED" ]]; then echo "expected packed $PACKED to be smaller than packed $OLDPACKED due to compression"; fi
