# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ MULTIPLEXED=2 default_setup_blobimport "blob_files"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting

Check that healer queue has all items
  $ read_blobstore_sync_queue_size
  90

Run the heal
  $ mononoke_blobstore_healer -q --iteration-limit=1 --heal-min-age-secs=0 --storage-id=blobstore --sync-queue-limit=100 2>&1 | strip_glog | grep -E -v "^(Monitoring|Discovered) regions:.*"
  Replication lag is * (glob)
  Fetched 90 queue entires (before building healing futures)
  Out of them 30 distinct blobstore keys, 30 distinct operation keys
  Found 30 blobs to be healed... Doing it with weight limit 10000000000, max concurrency: 100
  For 30 blobs did HealStats { queue_add: 0, queue_del: 90, put_success: 0, put_failure: 0 }
  Deleting 90 actioned queue entries
  Iteration rows processed: * rows, *s; total: * rows, *s (glob)
  The last batch was not full size, waiting...

Check that healer queue has drained
  $ read_blobstore_sync_queue_size
  0

Erase the sqllites and blobstore_sync_queue
  $ rm -rf "$TESTTMP/blobstore"
  $ erase_blobstore_sync_queue

blobimport them into Mononoke storage again, but with write failures on one side
  $ blobimport repo-hg/.hg repo --blobstore-write-chaos-rate=1

Check the stores have expected counts
  $ ls blobstore/0/blobs/ | wc -l
  0
  $ ls blobstore/1/blobs/ | wc -l
  30
  $ ls blobstore/2/blobs/ | wc -l
  30

Check that healer queue has successful items
  $ read_blobstore_sync_queue_size
  60

Run the heal, with write errors injected, simulating store still bad
  $ function count_log() {
  >  sed -re 's/^(Adding source blobstores \[BlobstoreId\(1\), BlobstoreId\(2\)\] to the queue so that failed destination blob stores \[BlobstoreId\(0\)\] will be retried later).*/\1/' |
  >  uniq -c | sed 's/^ *//'
  > }
  $ mononoke_blobstore_healer --blobstore-write-chaos-rate 1 -q --iteration-limit=1 --heal-min-age-secs=0 --storage-id=blobstore --sync-queue-limit=100 2>&1 | strip_glog | count_log | grep -v "speed" | grep -E -v "^1 (Monitoring|Discovered) regions:.*"
  1 Replication lag is * (glob)
  1 Fetched 60 queue entires (before building healing futures)
  1 Out of them 30 distinct blobstore keys, 30 distinct operation keys
  1 Found 30 blobs to be healed... Doing it with weight limit 10000000000, max concurrency: 100
  30 Adding source blobstores [BlobstoreId(1), BlobstoreId(2)] to the queue so that failed destination blob stores [BlobstoreId(0)] will be retried later
  1 For 30 blobs did HealStats { queue_add: 60, queue_del: 60, put_success: 60, put_failure: 30 }
  1 Deleting 60 actioned queue entries
  1 Iteration rows processed: * rows, *s; total: * rows, *s (glob)
  1 The last batch was not full size, waiting...

Check that healer queue still has the items, should not have drained
  $ read_blobstore_sync_queue_size
  60

Healer run again now store recovered
  $ mononoke_blobstore_healer -q --iteration-limit=1 --heal-min-age-secs=0 --storage-id=blobstore --sync-queue-limit=100 2>&1 | strip_glog | count_log | grep -E -v "^1 (Monitoring|Discovered) regions:.*"
  1 Replication lag is * (glob)
  1 Fetched 60 queue entires (before building healing futures)
  1 Out of them 30 distinct blobstore keys, 30 distinct operation keys
  1 Found 30 blobs to be healed... Doing it with weight limit 10000000000, max concurrency: 100
  1 For 30 blobs did HealStats { queue_add: 0, queue_del: 60, put_success: 30, put_failure: 0 }
  1 Deleting 60 actioned queue entries
  1 Iteration rows processed: * rows, *s; total: * rows, *s (glob)
  1 The last batch was not full size, waiting...

Check that healer queue has drained
  $ read_blobstore_sync_queue_size
  0

Check the stores have expected counts
  $ ls blobstore/0/blobs/ | wc -l
  30
  $ ls blobstore/1/blobs/ | wc -l
  30
  $ ls blobstore/2/blobs/ | wc -l
  30
