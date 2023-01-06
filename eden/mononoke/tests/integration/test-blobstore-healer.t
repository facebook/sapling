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

Check that healer queue has drained
  $ read_blobstore_wal_queue_size
  0

Erase the sqllites and blobstore_sync_queue
  $ rm -rf "$TESTTMP/blobstore"

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
  $ read_blobstore_wal_queue_size
  30

Run the heal, with write errors injected, simulating store still bad
  $ function count_log() {
  >  sed -re 's/^(Adding source blobstores \[BlobstoreId\(1\), BlobstoreId\(2\)\] to the queue so that failed destination blob stores \[BlobstoreId\(0\)\] will be retried later).*/\1/' |
  >  uniq -c | sed 's/^ *//'
  > }
  $ mononoke_blobstore_healer --blobstore-write-chaos-rate 1 -q --iteration-limit=1 --heal-min-age-secs=0 --storage-id=blobstore --sync-queue-limit=100 2>&1 | strip_glog | count_log | grep -v "speed" | grep -E -v "^1 (Monitoring|Discovered) regions:.*"
  1 Fetched 30 distinct put operations
  1 Found 30 blobs to be healed... Doing it with weight limit 10000000000, max concurrency: 100
  1 Couldn't heal blob repo0000.alias.gitsha1.7371f47a6f8bd23a8fa1a8b2a9479cdd76380e54 in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.alias.gitsha1.8c7e5a667f1b771847fe88c01c3de34413a1b220 in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.alias.gitsha1.96d80cd6c4e7158dbebd0849f4fb7ce513e5828c in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.alias.sha1.32096c2e0eff33d844ee6d675407ace18289357d in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.alias.sha1.6dcd4ce23d88e2ee9568ba546c007c63d9131c1b in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.alias.sha1.ae4f281df5a5d0ff3cad6371f76d5c29b6d953ec in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.alias.sha256.559aead08264d5795d3909718cdd05abd49572e84fe55590eef31a88a08fdffd in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.alias.sha256.6b23c0d5f35d1b11f9b683f0b0a617355deb11277d91ae091d399c655b87940d in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.alias.sha256.df7e70e5021544f4834bbee64a9e3789febc4be81470df629cad6ddb03320a5c in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.changeset.blake2.459f16ae564c501cb408c1e5b60fc98a1e8b8e97b9409c7520658bfa1577fb66 in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.changeset.blake2.9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.content.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.content.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.content.blake2.eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9 in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.content_metadata.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.content_metadata.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.content_metadata.blake2.eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9 in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.filenode_lookup.61585a6b75335f6ec9540101b7147908564f2699dcad59134fdf23cb086787ad in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.filenode_lookup.9915e555ad3fed014aa36a4e48549c1130fddffc7660589f42af5f0520f1118e in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.filenode_lookup.a0377040953a1a3762b7c59cb526797c1afd7ae6fcebb4d11e3c9186a56edb4e in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.hgchangeset.sha1.112478962961147124edd43549aedd1a335e44bf in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.hgchangeset.sha1.26805aba1e600a82e93661149f2313866a221a7b in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.hgchangeset.sha1.426bada5c67598ca65036d57d9e4b64b0c1ce7a0 in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.hgfilenode.sha1.005d992c5dcf32993668f7cede29d296c494a5d9 in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.hgfilenode.sha1.35e7525ce3a48913275d7061dd9a867ffef1e34d in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.hgfilenode.sha1.a2e456504a5e61f763f1a0b36a6c247c7541b2b3 in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.hgmanifest.sha1.41b34f08c1356f6ad068e9ab9b43d984245111aa in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.hgmanifest.sha1.7c9b4fd8b49377e2fead2e9610bb8db910a98c53 in these blobstores: {BlobstoreId(0)}
  1 Couldn't heal blob repo0000.hgmanifest.sha1.eb79886383871977bccdb3000c275a279f0d4c99 in these blobstores: {BlobstoreId(0)}
  1 For 30 processed entries and 30 blobstore keys: healthy blobs 0, healed blobs 0, failed to heal 30, missing blobs 0
  1 Requeueing 30 queue entries for another healing attempt
  1 Deleting 30 actioned queue entries
  1 Iteration rows processed: * rows, *s; total: * rows, *s (glob)
  1 The last batch was not full size, waiting...

Check that healer queue still has the items, should not have drained
  $ read_blobstore_wal_queue_size
  30

Healer run again now store recovered
  $ mononoke_blobstore_healer -q --iteration-limit=1 --heal-min-age-secs=0 --storage-id=blobstore --sync-queue-limit=100 2>&1 | strip_glog | count_log | grep -E -v "^1 (Monitoring|Discovered) regions:.*"
  1 Fetched 30 distinct put operations
  1 Found 30 blobs to be healed... Doing it with weight limit 10000000000, max concurrency: 100
  1 For 30 processed entries and 30 blobstore keys: healthy blobs 0, healed blobs 30, failed to heal 0, missing blobs 0
  1 Deleting 30 actioned queue entries
  1 Iteration rows processed: * rows, *s; total: * rows, *s (glob)
  1 The last batch was not full size, waiting...

Check that healer queue has drained
  $ read_blobstore_wal_queue_size
  0

Check the stores have expected counts
  $ ls blobstore/0/blobs/ | wc -l
  30
  $ ls blobstore/1/blobs/ | wc -l
  30
  $ ls blobstore/2/blobs/ | wc -l
  30
