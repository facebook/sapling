/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Tests for the Changesets store.

#![deny(warnings)]

use anyhow::Error;
use blobstore_sync_queue::{
    BlobstoreSyncQueue, BlobstoreSyncQueueEntry, OperationKey, SqlBlobstoreSyncQueue,
};
use context::CoreContext;
use fbinit::FacebookInit;
use metaconfig_types::{BlobstoreId, MultiplexId};
use mononoke_types::DateTime;
use sql_construct::SqlConstruct;
use uuid::Uuid;

#[fbinit::test]
fn test_simple(fb: FacebookInit) -> Result<(), Error> {
    let mut rt = tokio_compat::runtime::Runtime::new().unwrap();

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

    let entry0 = BlobstoreSyncQueueEntry::new(key0.clone(), bs0, mp, t0, op0.clone());
    let entry1 = BlobstoreSyncQueueEntry::new(key0.clone(), bs1, mp, t1, op0.clone());
    let entry2 = BlobstoreSyncQueueEntry::new(key1.clone(), bs0, mp, t1, op1.clone());
    let entry3 = BlobstoreSyncQueueEntry::new(key0.clone(), bs0, mp, t2, op2.clone());
    let entry4 = BlobstoreSyncQueueEntry::new(key0.clone(), bs1, mp, t2, op2);

    // add
    assert!(rt.block_on(queue.add(ctx.clone(), entry0.clone())).is_ok());
    assert!(rt.block_on(queue.add(ctx.clone(), entry1.clone())).is_ok());
    assert!(rt.block_on(queue.add(ctx.clone(), entry2.clone())).is_ok());
    assert!(rt.block_on(queue.add(ctx.clone(), entry3.clone())).is_ok());
    assert!(rt.block_on(queue.add(ctx.clone(), entry4.clone())).is_ok());

    // get
    let entries1 = rt
        .block_on(queue.get(ctx.clone(), key0.clone()))
        .expect("Get failed");
    assert_eq!(entries1.len(), 4);
    assert_eq!(entries1[0].operation_key, op0);
    let entries2 = rt
        .block_on(queue.get(ctx.clone(), key1.clone()))
        .expect("Get failed");
    assert_eq!(entries2.len(), 1);
    assert_eq!(entries2[0].operation_key, op1);

    // iter
    let some_entries = rt
        .block_on(queue.iter(ctx.clone(), None, mp, t1, 1))
        .expect("DateTime range iteration failed");
    assert_eq!(some_entries.len(), 2);
    let some_entries = rt
        .block_on(queue.iter(ctx.clone(), None, mp, t1, 2))
        .expect("DateTime range iteration failed");
    assert_eq!(some_entries.len(), 3);
    let some_entries = rt
        .block_on(queue.iter(ctx.clone(), None, mp, t0, 1))
        .expect("DateTime range iteration failed");
    assert_eq!(some_entries.len(), 2);
    let some_entries = rt
        .block_on(queue.iter(ctx.clone(), None, mp, t0, 100))
        .expect("DateTime range iteration failed");
    assert_eq!(some_entries.len(), 2);

    // delete
    rt.block_on(queue.del(ctx.clone(), vec![entry0]))
        .expect_err("Deleting entry without `id` should have failed");
    rt.block_on(queue.del(ctx.clone(), entries1))
        .expect("Failed to remove entries1");
    rt.block_on(queue.del(ctx.clone(), entries2))
        .expect("Failed to remove entries2");

    // iter
    let entries = rt
        .block_on(queue.iter(ctx.clone(), None, mp, t1, 100))
        .expect("Iterating over entries failed");
    assert_eq!(entries.len(), 0);
    Ok(())
}
