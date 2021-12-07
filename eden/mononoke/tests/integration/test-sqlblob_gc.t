# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ default_setup_blobimport "blob_sqlite"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting

Check that sqlblob has some data big enough to form a chunk
  $ for s in 0 1; do sqlite3 -readonly "$TESTTMP/blobstore/blobs/shard_${s}.sqlite" "SELECT COUNT(1) FROM chunk" ; done
  0
  1

Check that chunk_generations populated from put and they have length
  $ for s in 0 1; do sqlite3 -readonly "$TESTTMP/blobstore/blobs/shard_${s}.sqlite" "SELECT COUNT(1), last_seen_generation, value_len FROM chunk_generation" | sed "s/^/$s /"; done
  0 0||
  1 1|2|199

Run sqlblob_gc generation size report
  $ mononoke_sqlblob_gc --storage-config-name=blobstore --shard-count=2 generation-size 2>&1 | strip_glog
  Generation | Size
  -----------------
           2 | 199 B

Run sqlblob_gc mark, without populating value_len
  $ mononoke_sqlblob_gc --storage-config-name=blobstore --shard-count=2 mark --skip-missing-value-len 2>&1 | strip_glog
  Starting initial generation set
  Completed initial generation set
  Starting sweep
  Starting sweep on data keys from shard 0
  Starting sweep on data keys from shard 1
  Completed all sweeps

Run sqlblob_gc generation size report again, just to check mark has not broken it
  $ mononoke_sqlblob_gc --storage-config-name=blobstore --shard-count=2 generation-size 2>&1 | strip_glog
  Generation | Size
  -----------------
           2 | 199 B

Run sqlblob_gc mark
  $ mononoke_sqlblob_gc --storage-config-name=blobstore --shard-count=2 mark 2>&1 | strip_glog
  Starting populating missing value_len
  Completed populating missing value_len
  Starting initial generation set
  Completed initial generation set
  Starting sweep
  Starting sweep on data keys from shard 0
  Starting sweep on data keys from shard 1
  Completed all sweeps

Check that chunk_generations populated and they have length
  $ for s in 0 1; do sqlite3 -readonly "$TESTTMP/blobstore/blobs/shard_${s}.sqlite" "SELECT COUNT(1), last_seen_generation, value_len FROM chunk_generation" | sed "s/^/$s /"; done
  0 0||
  1 1|2|199

Run sqlblob_gc generation size report, the mark should have populated the length
  $ mononoke_sqlblob_gc --storage-config-name=blobstore --shard-count=2 generation-size 2>&1 | strip_glog
  Generation | Size
  -----------------
           2 | 199 B
