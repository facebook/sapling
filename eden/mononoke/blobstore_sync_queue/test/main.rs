/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::format_err;
use anyhow::Error;
use blobstore_sync_queue::BlobstoreWal;
use blobstore_sync_queue::BlobstoreWalEntry;
use blobstore_sync_queue::SqlBlobstoreWal;
use context::CoreContext;
use fbinit::FacebookInit;
use metaconfig_types::MultiplexId;
use mononoke_types::DateTime;
use sql_construct::SqlConstruct;

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

    let entry0 = BlobstoreWalEntry::new(key0.clone(), mp, t0, 12);
    let entry1 = BlobstoreWalEntry::new(key0, mp, t1, 13);
    let entry2 = BlobstoreWalEntry::new(key1.clone(), mp, t1, 14);
    let entry3 = BlobstoreWalEntry::new(key1, mp, t2, 15);

    // add
    wal.log(&ctx, entry0.clone()).await.unwrap();
    wal.log_many(&ctx, vec![entry1, entry2.clone()])
        .await
        .unwrap();
    wal.log(&ctx, entry3.clone()).await.unwrap();

    // read different ranges of entries
    let validate = |entry: &BlobstoreWalEntry, expected: &BlobstoreWalEntry| {
        assert_eq!(entry.blobstore_key, expected.blobstore_key);
        assert_eq!(entry.multiplex_id, expected.multiplex_id);
        assert_eq!(entry.blob_size, expected.blob_size);
        assert_eq!(entry.retry_count, expected.retry_count);
        assert!(entry.read_info.id.is_some());
        assert!(entry.read_info.shard_id.is_some());
        // read_info is not compared
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
