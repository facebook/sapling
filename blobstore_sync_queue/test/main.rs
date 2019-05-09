// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Tests for the Changesets store.

#![deny(warnings)]

use blobstore_sync_queue::{
    BlobstoreSyncQueue, BlobstoreSyncQueueEntry, SqlBlobstoreSyncQueue, SqlConstructors,
};
use context::CoreContext;
use metaconfig_types::BlobstoreId;
use mononoke_types::{DateTime, RepositoryId};

#[test]
fn test_simple() {
    let mut rt = tokio::runtime::Runtime::new().unwrap();

    let ctx = CoreContext::test_mock();
    let queue = SqlBlobstoreSyncQueue::with_sqlite_in_memory().unwrap();
    let repo_id = RepositoryId::new(137);
    let bs0 = BlobstoreId::new(0);
    let bs1 = BlobstoreId::new(1);

    let key0 = String::from("key0");
    let key1 = String::from("key1");
    let t0 = DateTime::from_rfc3339("2018-11-29T12:00:00.00Z").unwrap();
    let t1 = DateTime::from_rfc3339("2018-11-29T12:01:00.00Z").unwrap();
    let entry0 = BlobstoreSyncQueueEntry::new(repo_id, key0.clone(), bs0, t0);
    let entry1 = BlobstoreSyncQueueEntry::new(repo_id, key0.clone(), bs1, t1);
    let entry2 = BlobstoreSyncQueueEntry::new(repo_id, key1.clone(), bs0, t1);

    // add
    assert!(rt.block_on(queue.add(ctx.clone(), entry0.clone())).is_ok());
    assert!(rt.block_on(queue.add(ctx.clone(), entry1.clone())).is_ok());
    assert!(rt.block_on(queue.add(ctx.clone(), entry2.clone())).is_ok());

    // get
    let entries1 = rt
        .block_on(queue.get(ctx.clone(), repo_id, key0.clone()))
        .expect("Get failed");
    assert_eq!(entries1.len(), 2);
    let entries2 = rt
        .block_on(queue.get(ctx.clone(), repo_id, key1.clone()))
        .expect("Get failed");
    assert_eq!(entries2.len(), 1);

    // iter
    let some_entries = rt
        .block_on(queue.iter(ctx.clone(), repo_id, t1, 1))
        .expect("DateTime range iteration failed");
    assert_eq!(some_entries.len(), 2);
    let some_entries = rt
        .block_on(queue.iter(ctx.clone(), repo_id, t1, 2))
        .expect("DateTime range iteration failed");
    assert_eq!(some_entries.len(), 3);
    let some_entries = rt
        .block_on(queue.iter(ctx.clone(), repo_id, t0, 1))
        .expect("DateTime range iteration failed");
    assert_eq!(some_entries.len(), 2);
    let some_entries = rt
        .block_on(queue.iter(ctx.clone(), repo_id, t0, 100))
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
        .block_on(queue.iter(ctx.clone(), repo_id, t1, 100))
        .expect("Iterating over entries failed");
    assert_eq!(entries.len(), 0)
}
