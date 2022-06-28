/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::format_err;
use anyhow::Error;
use blobstore_sync_queue::BlobstoreSyncQueue;
use blobstore_sync_queue::BlobstoreSyncQueueEntry;
use blobstore_sync_queue::BlobstoreWal;
use blobstore_sync_queue::BlobstoreWalEntry;
use blobstore_sync_queue::OperationKey;
use blobstore_sync_queue::SqlBlobstoreSyncQueue;
use blobstore_sync_queue::SqlBlobstoreWal;
use context::CoreContext;
use fbinit::FacebookInit;
use metaconfig_types::BlobstoreId;
use metaconfig_types::MultiplexId;
use mononoke_types::DateTime;
use sql_construct::SqlConstruct;
use uuid::Uuid;

#[fbinit::test]
async fn test_sync_queue_simple(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let queue = SqlBlobstoreSyncQueue::with_sqlite_in_memory().unwrap();
    let bs0 = BlobstoreId::new(0);
    let bs1 = BlobstoreId::new(1);
    let mp = MultiplexId::new(1);

    let key0 = String::from("key0");
    let key1 = String::from("key1");
    let t0 = DateTime::from_rfc3339("2018-11-29T12:00:00.00Z").unwrap();
    let t1 = DateTime::from_rfc3339("2018-11-29T12:01:00.00Z").unwrap();
    let t2 = DateTime::from_rfc3339("2018-11-29T12:02:00.00Z").unwrap();

    let node_id = [1, 2, 2, 4, 5, 6, 7, 8];
    let op0 = OperationKey(Uuid::from_fields(0, 0, 1, &node_id)?); // for key0
    let op1 = OperationKey(Uuid::from_fields(0, 0, 2, &node_id)?); // for key1
    let op2 = OperationKey(Uuid::from_fields(0, 0, 3, &node_id)?); // for second put of key0

    let entry0 = BlobstoreSyncQueueEntry::new(key0.clone(), bs0, mp, t0, op0.clone(), None);
    let entry1 = BlobstoreSyncQueueEntry::new(key0.clone(), bs1, mp, t1, op0.clone(), None);
    let entry2 = BlobstoreSyncQueueEntry::new(key1.clone(), bs0, mp, t1, op1.clone(), None);
    let entry3 = BlobstoreSyncQueueEntry::new(key0.clone(), bs0, mp, t2, op2.clone(), None);
    let entry4 = BlobstoreSyncQueueEntry::new(key0.clone(), bs1, mp, t2, op2, None);

    // add
    assert!(queue.add(&ctx, entry0.clone()).await.is_ok());
    assert!(queue.add(&ctx, entry1.clone()).await.is_ok());
    assert!(queue.add(&ctx, entry2.clone()).await.is_ok());
    assert!(queue.add(&ctx, entry3.clone()).await.is_ok());
    assert!(queue.add(&ctx, entry4.clone()).await.is_ok());

    // get
    let entries1 = queue.get(&ctx, &key0).await.expect("Get failed");
    assert_eq!(entries1.len(), 4);
    assert_eq!(entries1[0].operation_key, op0);
    let entries2 = queue.get(&ctx, &key1).await.expect("Get failed");
    assert_eq!(entries2.len(), 1);
    assert_eq!(entries2[0].operation_key, op1);

    // iter
    let some_entries = queue
        .iter(&ctx, None, mp, t1, 1)
        .await
        .expect("DateTime range iteration failed");
    assert_eq!(some_entries.len(), 2);
    let some_entries = queue
        .iter(&ctx, None, mp, t1, 2)
        .await
        .expect("DateTime range iteration failed");
    assert_eq!(some_entries.len(), 3);
    let some_entries = queue
        .iter(&ctx, None, mp, t0, 1)
        .await
        .expect("DateTime range iteration failed");
    assert_eq!(some_entries.len(), 2);
    let some_entries = queue
        .iter(&ctx, None, mp, t0, 100)
        .await
        .expect("DateTime range iteration failed");
    assert_eq!(some_entries.len(), 2);

    // delete
    queue
        .del(&ctx, &[entry0])
        .await
        .expect_err("Deleting entry without `id` should have failed");
    queue
        .del(&ctx, &entries1)
        .await
        .expect("Failed to remove entries1");
    queue
        .del(&ctx, &entries2)
        .await
        .expect("Failed to remove entries2");

    // iter
    let entries = queue
        .iter(&ctx, None, mp, t1, 100)
        .await
        .expect("Iterating over entries failed");
    assert_eq!(entries.len(), 0);
    Ok(())
}

#[fbinit::test]
async fn test_write_ahead_log(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let wal = SqlBlobstoreWal::with_sqlite_in_memory()?;
    let mp = MultiplexId::new(1);

    let key0 = String::from("key0");
    let key1 = String::from("key1");
    let t0 = DateTime::from_rfc3339("2018-11-29T12:00:00.00Z")?.into();
    let t1 = DateTime::from_rfc3339("2018-11-29T12:01:00.00Z")?.into();
    let t2 = DateTime::from_rfc3339("2018-11-29T12:02:00.00Z")?.into();

    let node_id = [1, 2, 2, 4, 5, 6, 7, 8];
    // All operation keys are different because using WAL instead of a sync-queue
    // allows to write a key to the WAL only once.
    // If the key has multiple appearances in the WAL, it means it was written
    // in different sessions with different operation keys.
    let op0 = OperationKey(Uuid::from_fields(0, 0, 1, &node_id)?); // for key0
    let op1 = OperationKey(Uuid::from_fields(0, 0, 2, &node_id)?); // for second put of key0
    let op2 = OperationKey(Uuid::from_fields(0, 0, 3, &node_id)?); // for key1
    let op3 = OperationKey(Uuid::from_fields(0, 0, 4, &node_id)?); // for second put of key1

    let entry0 = BlobstoreWalEntry::new(key0.clone(), mp, t0, op0.clone(), None);
    let entry1 = BlobstoreWalEntry::new(key0, mp, t1, op1, None);
    let entry2 = BlobstoreWalEntry::new(key1.clone(), mp, t1, op2, None);
    let entry3 = BlobstoreWalEntry::new(key1, mp, t2, op3, None);

    // add
    assert!(wal.log(&ctx, entry0.clone()).await.is_ok());
    assert!(
        wal.log_many(&ctx, vec![entry1, entry2.clone()])
            .await
            .is_ok()
    );
    assert!(wal.log(&ctx, entry3.clone()).await.is_ok());

    // read different ranges of entries
    let validate = |entry: &BlobstoreWalEntry, expected: &BlobstoreWalEntry| {
        assert_eq!(entry.blobstore_key, expected.blobstore_key);
        assert_eq!(entry.multiplex_id, expected.multiplex_id);
        assert_eq!(entry.timestamp, expected.timestamp);
        assert_eq!(entry.operation_key, expected.operation_key);
        assert_eq!(entry.blob_size, expected.blob_size);
    };

    let some_entries = wal
        .read(&ctx, &mp, &t0, 1)
        .await
        .expect("DateTime range iteration failed");
    assert_eq!(some_entries.len(), 1);
    validate(
        some_entries
            .get(0)
            .ok_or_else(|| format_err!("must have entry"))?,
        &entry0,
    );

    let mut some_entries = wal
        .read(&ctx, &mp, &t1, 5)
        .await
        .expect("DateTime range iteration failed");
    assert_eq!(some_entries.len(), 3);
    some_entries.sort_by(|a, b| {
        if a.timestamp == b.timestamp {
            a.blobstore_key.cmp(&b.blobstore_key)
        } else {
            a.timestamp.cmp(&b.timestamp)
        }
    });
    validate(
        some_entries
            .get(2)
            .ok_or_else(|| format_err!("must have entry"))?,
        &entry2,
    );

    let mut some_entries = wal
        .read(&ctx, &mp, &t2, 7)
        .await
        .expect("DateTime range iteration failed");
    assert_eq!(some_entries.len(), 4);
    some_entries.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    validate(
        some_entries
            .get(3)
            .ok_or_else(|| format_err!("must have entry"))?,
        &entry3,
    );

    let some_entries = wal
        .read(&ctx, &mp, &t2, 0)
        .await
        .expect("DateTime range iteration failed");
    assert!(some_entries.is_empty());

    Ok(())
}
