# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ MULTIPLEXED=1 PACK_BLOB=0 setup_common_config "blob_files"
  $ cd "$TESTTMP"
  $ testtool_drawdag -R repo <<'EOF'
  > C
  > |
  > B
  > |
  > A
  > # modify: C "C" pad=10000 "C"
  > # modify: B "B" pad=10000 "B"
  > # modify: A "A" pad=10000 "A"
  > EOF
  A=b7150acb604b51b1d5786f0f7ea951d8ffe902b7313bdeba9620b2ead15e7d66
  B=fab9d974bdf31ffcc908fc5375502d4d38467b091144512700bd8b023982ab57
  C=c2351942da4871c51880dc56e8371474a4f0e82884c62c45581781daede54887

Check the stores have expected counts
  $ ls blobstore/0/blobs/ | wc -l
  21
  $ ls blobstore/1/blobs/ | wc -l
  21

Check that the packed sizes are larger due to the packblob wrappers on store 0
  $ PACKED=$(du -s --bytes blobstore/0/blobs/ | cut -f1); UNPACKED=$(du -s --bytes blobstore/1/blobs/ | cut -f1)
  $ if [[ "$PACKED" -le "$UNPACKED" ]]; then echo "expected packed $PACKED to be larger than unpacked $UNPACKED due to thift wrappers"; fi

Move the uncompressed packed store aside
  $ mv "$TESTTMP/blobstore/0" "$TESTTMP/blobstore.raw"
  $ rm -rf "$TESTTMP/blobstore_sync_queue/sqlite_dbs" "$TESTTMP/blobstore"

Import again with zstd compression
  $ MULTIPLEXED=1 PACK_BLOB=1 setup_common_config "blob_files"
  $ cd "$TESTTMP"
  $ cat > mononoke-config/common/storage.toml <<EOF
  > # Start new config
  > [blobstore.metadata.local]
  > local_db_path = "$TESTTMP/monsql"
  > [blobstore.blobstore.multiplexed_wal]
  > multiplex_id = 1
  > queue_db = { local = { local_db_path = "$TESTTMP/blobstore_sync_queue" } }
  > write_quorum = 1
  > multiplex_scuba_table = "file://$TESTTMP/blobstore_trace_scuba.json"
  > components = [
  >   { blobstore_id = 0, blobstore = { pack = { blobstore = { blob_files = { path = "$TESTTMP/blobstore/0" } }, pack_config = { put_format = { ZstdIndividual = { compression_level = 3 } } } } } },
  >   { blobstore_id = 1, blobstore = { pack = { blobstore = { blob_files = { path = "$TESTTMP/blobstore/1" } }, pack_config = { put_format = { ZstdIndividual = { compression_level = 3 } } } } } },
  > ]
  > [blobstore.mutable_blobstore]
  > blob_files = { path = "$TESTTMP/blobstore/mutable" }
  > EOF
  $ testtool_drawdag -R repo <<'EOF'
  > C
  > |
  > B
  > |
  > A
  > # modify: C "C" pad=10000 "C"
  > # modify: B "B" pad=10000 "B"
  > # modify: A "A" pad=10000 "A"
  > EOF
  A=b7150acb604b51b1d5786f0f7ea951d8ffe902b7313bdeba9620b2ead15e7d66
  B=fab9d974bdf31ffcc908fc5375502d4d38467b091144512700bd8b023982ab57
  C=c2351942da4871c51880dc56e8371474a4f0e82884c62c45581781daede54887

Check that the packed sizes are smaller due to compression
  $ PACKED=$(du -s --bytes blobstore/0/blobs/ | cut -f1); OLDPACKED=$(du -s --bytes blobstore.raw/blobs/ | cut -f1)
  $ if [[ "$PACKED" -ge "$OLDPACKED" ]]; then echo "expected packed $PACKED to be smaller than packed $OLDPACKED due to compression"; fi
