/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreBytes;
use blobstore::BlobstoreGetData;
use blobstore_sync_queue::SqlBlobstoreWal;
use bytes::Bytes;
use context::CoreContext;
use fbinit::FacebookInit;
use futures_03_ext::BufferedParams;
use metaconfig_types::BlobstoreId;
use metaconfig_types::MultiplexId;
use sql_construct::SqlConstruct;

use super::*;
use crate::wal_healer::WalHealer;

#[derive(Clone, Debug, Default)]
struct GoodBlob {
    inner: Arc<Mutex<HashMap<String, BlobstoreBytes>>>,
}

impl std::fmt::Display for GoodBlob {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "GoodBlob")
    }
}

#[async_trait]
impl Blobstore for GoodBlob {
    async fn put<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        let mut inner = self.inner.lock().expect("lock poison");
        inner.insert(key, value);
        Ok(())
    }

    async fn get<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        let inner = self.inner.lock().expect("lock poison");
        let bytes = inner.get(key).map(|bytes| bytes.clone().into());
        Ok(bytes)
    }
}

#[derive(Clone, Debug)]
struct FailingBlob;

impl std::fmt::Display for FailingBlob {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "FailingBlob")
    }
}

#[async_trait]
impl Blobstore for FailingBlob {
    async fn put<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        _key: String,
        _value: BlobstoreBytes,
    ) -> Result<()> {
        anyhow::bail!("Failed put!");
    }

    async fn get<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        _key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        anyhow::bail!("Failed get!");
    }
}

#[fbinit::test]
async fn test_all_blobstores_failing(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let bs1: Arc<dyn Blobstore> = Arc::new(FailingBlob);
    let bs2 = Arc::new(FailingBlob);
    let bs3 = Arc::new(FailingBlob);
    let blobstores: Arc<HashMap<_, _>> = Arc::new(
        vec![
            (BlobstoreId::new(1), bs1.clone()),
            (BlobstoreId::new(2), bs2.clone()),
            (BlobstoreId::new(3), bs3.clone()),
        ]
        .into_iter()
        .collect(),
    );

    // set up some variables
    let multiplex_id = MultiplexId::new(1);

    let ts = Timestamp::now();
    let key = "key".to_string();

    // all blobatores fail, so it doesn't matter whether we actually write the blob or not

    // the queue will have an entry for the previous write
    let wal = Arc::new(SqlBlobstoreWal::with_sqlite_in_memory()?);
    let entry = BlobstoreWalEntry::new(key.clone(), multiplex_id, ts, 12);
    wal.log_many(&ctx, vec![entry]).await?;

    let expected = vec![key.clone()];
    validate_queue(&ctx, wal.clone(), multiplex_id, Timestamp::now(), expected).await?;

    let buf_params = BufferedParams {
        weight_limit: 1000,
        buffer_size: 100,
    };
    let healer = WalHealer::new(10, buf_params, wal.clone(), blobstores, multiplex_id, false);

    let age = ChronoDuration::seconds(0);
    healer.heal(&ctx, age).await?;

    // check that the queue have the entry, because the blob couldn't be healed
    let expected = vec![key];
    validate_queue(&ctx, wal.clone(), multiplex_id, Timestamp::now(), expected).await?;

    Ok(())
}

#[fbinit::test]
async fn test_healthy_blob(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let bs1: Arc<dyn Blobstore> = Arc::new(GoodBlob::default());
    let bs2 = Arc::new(GoodBlob::default());
    let bs3 = Arc::new(GoodBlob::default());
    let blobstores: Arc<HashMap<_, _>> = Arc::new(
        vec![
            (BlobstoreId::new(1), bs1.clone()),
            (BlobstoreId::new(2), bs2.clone()),
            (BlobstoreId::new(3), bs3.clone()),
        ]
        .into_iter()
        .collect(),
    );

    // set up some variables
    let multiplex_id = MultiplexId::new(1);

    let ts = Timestamp::now();
    let key = "key".to_string();
    let value = make_value("value");

    // make sure blob is available in each of the blobstores
    bs1.put(&ctx, key.clone(), value.clone()).await?;
    bs2.put(&ctx, key.clone(), value.clone()).await?;
    bs3.put(&ctx, key.clone(), value.clone()).await?;

    // the queue will have an entry for the previous write
    // it can even have multiple entries, if the blob was written twice
    let wal = Arc::new(SqlBlobstoreWal::with_sqlite_in_memory()?);
    let entry1 = BlobstoreWalEntry::new(key.clone(), multiplex_id, ts, 13);
    let entry2 = BlobstoreWalEntry::new(key.clone(), multiplex_id, ts, 14);
    wal.log_many(&ctx, vec![entry1, entry2]).await?;

    let expected = vec![key.clone(), key];
    validate_queue(&ctx, wal.clone(), multiplex_id, Timestamp::now(), expected).await?;

    let buf_params = BufferedParams {
        weight_limit: 1000,
        buffer_size: 100,
    };
    let healer = WalHealer::new(10, buf_params, wal.clone(), blobstores, multiplex_id, false);

    let age = ChronoDuration::seconds(0);
    healer.heal(&ctx, age).await?;

    // check that the queue doesn't have any entries, because the blob is healthy
    validate_queue(&ctx, wal.clone(), multiplex_id, Timestamp::now(), vec![]).await?;

    Ok(())
}

#[fbinit::test]
async fn test_missing_blob_healed(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let bs1: Arc<dyn Blobstore> = Arc::new(GoodBlob::default());
    let bs2 = Arc::new(GoodBlob::default());
    let bs3 = Arc::new(GoodBlob::default());
    let blobstores: Arc<HashMap<_, _>> = Arc::new(
        vec![
            (BlobstoreId::new(1), bs1.clone()),
            (BlobstoreId::new(2), bs2.clone()),
            (BlobstoreId::new(3), bs3.clone()),
        ]
        .into_iter()
        .collect(),
    );

    // set up some variables
    let multiplex_id = MultiplexId::new(1);

    let ts = Timestamp::now();
    let key = "key".to_string();
    let value = make_value("value");

    // make sure blob is available at least in one of the blobstores
    bs1.put(&ctx, key.clone(), value.clone()).await?;

    // the queue will have an entry for the previous write
    let wal = Arc::new(SqlBlobstoreWal::with_sqlite_in_memory()?);
    let entry = BlobstoreWalEntry::new(key.clone(), multiplex_id, ts, 15);
    wal.log_many(&ctx, vec![entry]).await?;

    let expected = vec![key];
    validate_queue(&ctx, wal.clone(), multiplex_id, Timestamp::now(), expected).await?;

    let buf_params = BufferedParams {
        weight_limit: 1000,
        buffer_size: 100,
    };
    let healer = WalHealer::new(10, buf_params, wal.clone(), blobstores, multiplex_id, false);

    let age = ChronoDuration::seconds(0);
    healer.heal(&ctx, age).await?;

    // check that the queue is empty, the blob was healed
    validate_queue(&ctx, wal.clone(), multiplex_id, Timestamp::now(), vec![]).await?;

    Ok(())
}

#[fbinit::test]
async fn test_missing_blob_not_healed(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let bs1: Arc<dyn Blobstore> = Arc::new(GoodBlob::default());
    let bs2 = Arc::new(GoodBlob::default());
    let bs3: Arc<dyn Blobstore> = Arc::new(FailingBlob);
    let blobstores: Arc<HashMap<_, _>> = Arc::new(
        vec![
            (BlobstoreId::new(1), bs1.clone()),
            (BlobstoreId::new(2), bs2.clone()),
            (BlobstoreId::new(3), bs3.clone()),
        ]
        .into_iter()
        .collect(),
    );

    // set up some variables
    let multiplex_id = MultiplexId::new(1);

    let ts = Timestamp::now();
    let key = "key".to_string();
    let value = make_value("value");

    // make sure blob is available at least in one of the blobstores
    bs1.put(&ctx, key.clone(), value.clone()).await?;

    // the queue will have an entry for the previous write
    let wal = Arc::new(SqlBlobstoreWal::with_sqlite_in_memory()?);
    let entry = BlobstoreWalEntry::new(key.clone(), multiplex_id, ts, 16);
    wal.log_many(&ctx, vec![entry]).await?;

    let expected = vec![key.clone()];
    validate_queue(&ctx, wal.clone(), multiplex_id, Timestamp::now(), expected).await?;

    let buf_params = BufferedParams {
        weight_limit: 1000,
        buffer_size: 100,
    };
    let healer = WalHealer::new(10, buf_params, wal.clone(), blobstores, multiplex_id, false);

    let age = ChronoDuration::seconds(0);
    healer.heal(&ctx, age).await?;

    // check that the queue still has the entry, because the blob couldn't be healed
    // in the failing blobstore
    let expected = vec![key];
    validate_queue(&ctx, wal.clone(), multiplex_id, Timestamp::now(), expected).await?;

    Ok(())
}

#[fbinit::test]
async fn test_blob_cannot_be_fetched(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let bs1: Arc<dyn Blobstore> = Arc::new(GoodBlob::default());
    let bs2 = Arc::new(GoodBlob::default());
    let bs3: Arc<dyn Blobstore> = Arc::new(FailingBlob);
    let blobstores: Arc<HashMap<_, _>> = Arc::new(
        vec![
            (BlobstoreId::new(1), bs1.clone()),
            (BlobstoreId::new(2), bs2.clone()),
            (BlobstoreId::new(3), bs3.clone()),
        ]
        .into_iter()
        .collect(),
    );

    // set up some variables
    let multiplex_id = MultiplexId::new(1);

    let ts = Timestamp::now();
    let key = "key".to_string();

    // we assume the blob is only available in the failing blobstore

    // the queue will have an entry for the previous write
    let wal = Arc::new(SqlBlobstoreWal::with_sqlite_in_memory()?);
    let entry = BlobstoreWalEntry::new(key.clone(), multiplex_id, ts, 17);
    wal.log_many(&ctx, vec![entry]).await?;

    let expected = vec![key.clone()];
    validate_queue(&ctx, wal.clone(), multiplex_id, Timestamp::now(), expected).await?;

    let buf_params = BufferedParams {
        weight_limit: 1000,
        buffer_size: 100,
    };
    let healer = WalHealer::new(10, buf_params, wal.clone(), blobstores, multiplex_id, false);

    let age = ChronoDuration::seconds(0);
    healer.heal(&ctx, age).await?;

    // check that the queue still has the entry, because the blob couldn't be healed
    let expected = vec![key];
    validate_queue(&ctx, wal.clone(), multiplex_id, Timestamp::now(), expected).await?;

    Ok(())
}

#[fbinit::test]
async fn test_different_blobs_wal_entries(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let bs1: Arc<dyn Blobstore> = Arc::new(GoodBlob::default());
    let bs2 = Arc::new(GoodBlob::default());
    let bs3 = Arc::new(GoodBlob::default());
    let blobstores: Arc<HashMap<_, _>> = Arc::new(
        vec![
            (BlobstoreId::new(1), bs1.clone()),
            (BlobstoreId::new(2), bs2.clone()),
            (BlobstoreId::new(3), bs3.clone()),
        ]
        .into_iter()
        .collect(),
    );

    // set up some variables
    let mid1 = MultiplexId::new(1);
    let mid2 = MultiplexId::new(2);

    let ts = Timestamp::now();
    let key1 = "key1".to_string();
    let value1 = make_value("value1");
    let key2 = "key2".to_string();
    let value2 = make_value("value2");
    let key3 = "key3".to_string();
    let value3 = make_value("value3");

    // first blob will be available in 1 blobstore
    bs1.put(&ctx, key1.clone(), value1.clone()).await?;
    // second blob will be available in 2 blobstore
    bs2.put(&ctx, key2.clone(), value2.clone()).await?;
    // third blob will be available in 3 blobstore
    bs3.put(&ctx, key3.clone(), value3.clone()).await?;

    // the queue will have an entry for the previous writes and some other
    // entries from different multiplex configuration
    let wal = Arc::new(SqlBlobstoreWal::with_sqlite_in_memory()?);

    let entry1 = BlobstoreWalEntry::new(key1.clone(), mid1, ts, 18);
    let entry2 = BlobstoreWalEntry::new(key2.clone(), mid1, ts, 19);
    let entry3 = BlobstoreWalEntry::new(key3.clone(), mid2, ts, 20);

    wal.log_many(&ctx, vec![entry1, entry2, entry3]).await?;

    let expected = vec![key1, key2];
    validate_queue(&ctx, wal.clone(), mid1, Timestamp::now(), expected).await?;
    let expected = vec![key3.clone()];
    validate_queue(&ctx, wal.clone(), mid2, Timestamp::now(), expected).await?;

    let buf_params = BufferedParams {
        weight_limit: 1000,
        buffer_size: 100,
    };
    let healer = WalHealer::new(10, buf_params, wal.clone(), blobstores, mid1, false);

    let age = ChronoDuration::seconds(0);
    healer.heal(&ctx, age).await?;

    // check that the queue has only entries from different multiplex configuration
    validate_queue(&ctx, wal.clone(), mid1, Timestamp::now(), vec![]).await?;
    let expected = vec![key3.clone()];
    validate_queue(&ctx, wal.clone(), mid2, Timestamp::now(), expected).await?;

    // also check that the third blob wasn't healed
    assert!(bs1.get(&ctx, &key3).await?.is_none());
    assert!(bs2.get(&ctx, &key3).await?.is_none());

    Ok(())
}

#[fbinit::test]
async fn test_blob_missing_completely(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let bs1: Arc<dyn Blobstore> = Arc::new(GoodBlob::default());
    let bs2 = Arc::new(GoodBlob::default());
    let bs3 = Arc::new(GoodBlob::default());
    let blobstores: Arc<HashMap<_, _>> = Arc::new(
        vec![
            (BlobstoreId::new(1), bs1.clone()),
            (BlobstoreId::new(2), bs2.clone()),
            (BlobstoreId::new(3), bs3.clone()),
        ]
        .into_iter()
        .collect(),
    );

    // set up some variables
    let multiplex_id = MultiplexId::new(1);

    let ts = Timestamp::now();
    let key = "key".to_string();

    // the queue will have an entry for the previous write
    // it can even have multiple entries, if the blob was written twice
    let wal = Arc::new(SqlBlobstoreWal::with_sqlite_in_memory()?);
    let entry = BlobstoreWalEntry::new(key.clone(), multiplex_id, ts, 21);
    wal.log_many(&ctx, vec![entry]).await?;

    let expected = vec![key.clone()];
    validate_queue(&ctx, wal.clone(), multiplex_id, Timestamp::now(), expected).await?;

    let buf_params = BufferedParams {
        weight_limit: 1000,
        buffer_size: 100,
    };
    let healer = WalHealer::new(10, buf_params, wal.clone(), blobstores, multiplex_id, false);

    let age = ChronoDuration::seconds(0);
    healer.heal(&ctx, age).await?;

    // check that the queue has the entry, because the blob is completely
    // missing (all blobstore reads succeeded, but couldn't find the blob) and we are
    // unable to heal it now, but maybe it wasn't yet written to the storages
    let expected = vec![key];
    validate_queue(&ctx, wal.clone(), multiplex_id, Timestamp::now(), expected).await?;

    Ok(())
}

#[fbinit::test]
async fn test_entry_timestamp_updated(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let bs1: Arc<dyn Blobstore> = Arc::new(GoodBlob::default());
    let bs2: Arc<dyn Blobstore> = Arc::new(FailingBlob);
    let blobstores: Arc<HashMap<_, _>> = Arc::new(
        vec![
            (BlobstoreId::new(1), bs1.clone()),
            (BlobstoreId::new(2), bs2.clone()),
        ]
        .into_iter()
        .collect(),
    );

    // set up some variables
    let multiplex_id = MultiplexId::new(1);

    let original_ts = Timestamp::now();
    let key = "key".to_string();
    let value = make_value("value");

    bs1.put(&ctx, key.clone(), value.clone()).await?;

    // the queue will have an entry for the previous write
    let wal = Arc::new(SqlBlobstoreWal::with_sqlite_in_memory()?);
    let entry = BlobstoreWalEntry::new(key.clone(), multiplex_id, original_ts, 22);
    wal.log_many(&ctx, vec![entry]).await?;

    let original_entries = wal
        .read(&ctx, &multiplex_id, &Timestamp::now(), 100)
        .await?;
    assert!(original_entries.len() == 1);
    let original_entry = &original_entries[0];
    assert_eq!(original_ts, original_entry.timestamp);

    let buf_params = BufferedParams {
        weight_limit: 1000,
        buffer_size: 100,
    };
    let healer = WalHealer::new(10, buf_params, wal.clone(), blobstores, multiplex_id, false);

    let age = ChronoDuration::seconds(0);
    healer.heal(&ctx, age).await?;

    // check that the queue entry has a new timestamp
    let new_entries = wal
        .read(&ctx, &multiplex_id, &Timestamp::now(), 100)
        .await?;
    assert!(new_entries.len() == 1);
    let new_entry = &new_entries[0];
    assert!(original_entry.timestamp < new_entry.timestamp);

    Ok(())
}

async fn validate_queue<'a>(
    ctx: &'a CoreContext,
    wal: Arc<dyn BlobstoreWal>,
    multiplex_id: MultiplexId,
    older_than: Timestamp,
    mut expected: Vec<String>,
) -> Result<()> {
    let mut entries: Vec<_> = wal
        .read(ctx, &multiplex_id, &older_than, 100)
        .await?
        .into_iter()
        .map(|e| e.blobstore_key)
        .collect();

    assert_eq!(entries.len(), expected.len());

    entries.sort();
    expected.sort();

    for (a, b) in entries.iter().zip(expected.iter()) {
        assert_eq!(a, b);
    }

    Ok(())
}

fn make_value(value: &str) -> BlobstoreBytes {
    BlobstoreBytes::from_bytes(Bytes::copy_from_slice(value.as_bytes()))
}
