# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ MULTIPLEXED=2 setup_common_config "blob_files"
  $ cd "$TESTTMP"

Create repo using testtool
  $ testtool_drawdag -R repo <<EOF
  > C
  > |
  > B
  > |
  > A
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2

Check that healer queue has drained
  $ read_blobstore_wal_queue_size
  0

Populate WAL queue by simulating write failures - delete blobs from blobstore 0 and populate WAL
  $ mononoke_testtool populate-wal -R repo --blobstore-path "$TESTTMP/blobstore" --source-blobstore-id 1 --delete-target-blobs --storage-id=blobstore
  Found 21 blobs in source blobstore 1
  Deleted 21 blobs from target blobstore 0
  Inserted 21 WAL entries for target multiplex_id 1

Check the stores have expected counts
  $ ls blobstore/0/blobs/ | wc -l
  0
  $ ls blobstore/1/blobs/ | wc -l
  21
  $ ls blobstore/2/blobs/ | wc -l
  21

Check that healer queue has successful items
  $ read_blobstore_wal_queue_size
  21

Run the heal, with write errors injected, simulating store still bad
  $ function count_log() {
  >  sed -re 's/^(Adding source blobstores \[BlobstoreId\(1\), BlobstoreId\(2\)\] to the queue so that failed destination blob stores \[BlobstoreId\(0\)\] will be retried later).*/\1/' |
  >  uniq -c | sed 's/^ *//'
  > }
  $ mononoke_blobstore_healer --blobstore-write-chaos-rate 1 -q --iteration-limit=1 --heal-min-age-secs=0 --storage-id=blobstore --sync-queue-limit=100 2>&1 | count_log | grep -v "speed" | grep -E -v "^1 (Monitoring|Discovered) regions:.*"
  1 [INFO] Fetched 21 distinct put operations
  1 [INFO] Found 21 blobs to be healed... Doing it with weight limit 10000000000, max concurrency: 100
  1 [INFO] Couldn't heal blob repo0000.alias.gitsha1.7371f47a6f8bd23a8fa1a8b2a9479cdd76380e54 in these blobstores: {BlobstoreId(0)}
  1 [INFO] Couldn't heal blob repo0000.alias.gitsha1.8c7e5a667f1b771847fe88c01c3de34413a1b220 in these blobstores: {BlobstoreId(0)}
  1 [INFO] Couldn't heal blob repo0000.alias.gitsha1.96d80cd6c4e7158dbebd0849f4fb7ce513e5828c in these blobstores: {BlobstoreId(0)}
  1 [INFO] Couldn't heal blob repo0000.alias.seeded_blake3.5667f2421ac250c4bb9af657b5ead3cdbd940bfbc350b2bfee47454643832b48 in these blobstores: {BlobstoreId(0)}
  1 [INFO] Couldn't heal blob repo0000.alias.seeded_blake3.5ad3ba58a716e5fc04296ac9af7a1420f726b401fdf16d270beb5b6b30bc0cda in these blobstores: {BlobstoreId(0)}
  1 [INFO] Couldn't heal blob repo0000.alias.seeded_blake3.6fb4c384e79ac0771a483fcf3c46fb4ea8609f79608e8bcbf710f9887a3b9cf6 in these blobstores: {BlobstoreId(0)}
  1 [INFO] Couldn't heal blob repo0000.alias.sha1.32096c2e0eff33d844ee6d675407ace18289357d in these blobstores: {BlobstoreId(0)}
  1 [INFO] Couldn't heal blob repo0000.alias.sha1.6dcd4ce23d88e2ee9568ba546c007c63d9131c1b in these blobstores: {BlobstoreId(0)}
  1 [INFO] Couldn't heal blob repo0000.alias.sha1.ae4f281df5a5d0ff3cad6371f76d5c29b6d953ec in these blobstores: {BlobstoreId(0)}
  1 [INFO] Couldn't heal blob repo0000.alias.sha256.559aead08264d5795d3909718cdd05abd49572e84fe55590eef31a88a08fdffd in these blobstores: {BlobstoreId(0)}
  1 [INFO] Couldn't heal blob repo0000.alias.sha256.6b23c0d5f35d1b11f9b683f0b0a617355deb11277d91ae091d399c655b87940d in these blobstores: {BlobstoreId(0)}
  1 [INFO] Couldn't heal blob repo0000.alias.sha256.df7e70e5021544f4834bbee64a9e3789febc4be81470df629cad6ddb03320a5c in these blobstores: {BlobstoreId(0)}
  1 [INFO] Couldn't heal blob repo0000.changeset.blake2.aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675 in these blobstores: {BlobstoreId(0)}
  1 [INFO] Couldn't heal blob repo0000.changeset.blake2.e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2 in these blobstores: {BlobstoreId(0)}
  1 [INFO] Couldn't heal blob repo0000.changeset.blake2.f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658 in these blobstores: {BlobstoreId(0)}
  1 [INFO] Couldn't heal blob repo0000.content.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f in these blobstores: {BlobstoreId(0)}
  1 [INFO] Couldn't heal blob repo0000.content.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d in these blobstores: {BlobstoreId(0)}
  1 [INFO] Couldn't heal blob repo0000.content.blake2.eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9 in these blobstores: {BlobstoreId(0)}
  1 [INFO] Couldn't heal blob repo0000.content_metadata2.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f in these blobstores: {BlobstoreId(0)}
  1 [INFO] Couldn't heal blob repo0000.content_metadata2.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d in these blobstores: {BlobstoreId(0)}
  1 [INFO] Couldn't heal blob repo0000.content_metadata2.blake2.eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9 in these blobstores: {BlobstoreId(0)}
  1 [INFO] For 21 processed entries and 21 blobstore keys: healthy blobs 0, healed blobs 0, failed to heal 21, missing blobs 0
  1 [INFO] Requeuing 21 queue entries for another healing attempt
  1 [INFO] Deleting 21 actioned queue entries
  1 [INFO] Iteration rows processed: * rows, *s; total: * rows, *s (glob)
  1 [INFO] The last batch was not full size, waiting...

Check that healer queue still has the items, should not have drained
  $ read_blobstore_wal_queue_size
  21

Healer run again now store recovered
  $ mononoke_blobstore_healer -q --iteration-limit=1 --heal-min-age-secs=0 --storage-id=blobstore --sync-queue-limit=100 2>&1 | count_log | grep -E -v "^1 (Monitoring|Discovered) regions:.*"
  1 [INFO] Fetched 21 distinct put operations
  1 [INFO] Found 21 blobs to be healed... Doing it with weight limit 10000000000, max concurrency: 100
  1 [INFO] For 21 processed entries and 21 blobstore keys: healthy blobs 0, healed blobs 21, failed to heal 0, missing blobs 0
  1 [INFO] Deleting 21 actioned queue entries
  1 [INFO] Iteration rows processed: * rows, *s; total: * rows, *s (glob)
  1 [INFO] The last batch was not full size, waiting...

Check that healer queue has drained
  $ read_blobstore_wal_queue_size
  0

Check the stores have expected counts
  $ ls blobstore/0/blobs/ | wc -l
  21
  $ ls blobstore/1/blobs/ | wc -l
  21
  $ ls blobstore/2/blobs/ | wc -l
  21
