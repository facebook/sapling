/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Mutex;

use anyhow::format_err;
use async_trait::async_trait;
use blobstore::BlobstoreGetData;
use blobstore_sync_queue::SqlBlobstoreSyncQueue;
use bytes::Bytes;
use fbinit::FacebookInit;
use sql_construct::SqlConstruct;

use super::*;

// In-memory "blob store"
///
/// Pure in-memory implementation for testing, with put failure
#[derive(Clone, Debug)]
pub struct PutFailingMemblob {
    hash: Arc<Mutex<HashMap<String, BlobstoreBytes>>>,
    fail_puts: Arc<Mutex<bool>>,
}

impl std::fmt::Display for PutFailingMemblob {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "PutFailingMemblob")
    }
}

impl PutFailingMemblob {
    pub fn new() -> Self {
        Self {
            hash: Arc::new(Mutex::new(HashMap::new())),
            fail_puts: Arc::new(Mutex::new(false)),
        }
    }
    pub fn len(&self) -> usize {
        let inner = self.hash.lock().expect("lock poison");
        inner.len()
    }
    pub fn fail_puts(&self) {
        let mut data = self.fail_puts.lock().expect("lock poison");
        *data = true;
    }
    pub fn unfail_puts(&self) {
        let mut data = self.fail_puts.lock().expect("lock poison");
        *data = false;
    }
}

#[async_trait]
impl Blobstore for PutFailingMemblob {
    async fn put<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        let mut inner = self.hash.lock().expect("lock poison");
        let inner_flag = self.fail_puts.lock().expect("lock poison");
        if *inner_flag {
            Err(format_err!("Put failed for key"))
        } else {
            inner.insert(key, value);
            Ok(())
        }
    }

    async fn get<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        let inner = self.hash.lock().expect("lock poison");
        let bytes = inner.get(key).map(|bytes| bytes.clone().into());
        Ok(bytes)
    }
}

#[async_trait]
trait BlobstoreSyncQueueExt {
    async fn len<'out>(
        &'out self,
        ctx: &'out CoreContext,
        multiplex_id: MultiplexId,
    ) -> Result<usize>;
}

#[async_trait]
impl<Q: BlobstoreSyncQueue> BlobstoreSyncQueueExt for Q {
    async fn len<'out>(
        &'out self,
        ctx: &'out CoreContext,
        multiplex_id: MultiplexId,
    ) -> Result<usize> {
        let zero_date = DateTime::now();
        let entries = self.iter(ctx, None, multiplex_id, zero_date, 100).await?;
        if entries.len() >= 100 {
            Err(format_err!("too many entries"))
        } else {
            Ok(entries.len())
        }
    }
}

fn make_empty_stores(
    n: usize,
) -> (
    Vec<BlobstoreId>,
    HashMap<BlobstoreId, Arc<PutFailingMemblob>>,
    Arc<HashMap<BlobstoreId, Arc<dyn Blobstore>>>,
) {
    let mut test_bids = Vec::new();
    let mut test_stores = HashMap::new();
    let mut underlying_stores = HashMap::new();
    for i in 0..n {
        test_bids.push(BlobstoreId::new(i as u64));
        let u = Arc::new(PutFailingMemblob::new());
        let s: Arc<dyn Blobstore> = u.clone();
        test_stores.insert(test_bids[i], s);
        underlying_stores.insert(test_bids[i], u);
    }
    let stores = Arc::new(test_stores);
    // stores loses its concrete typing, so return underlying to allow access to len() etc.
    (test_bids, underlying_stores, stores)
}

fn make_value(value: &str) -> BlobstoreBytes {
    BlobstoreBytes::from_bytes(Bytes::copy_from_slice(value.as_bytes()))
}

async fn put_value(ctx: &CoreContext, store: Option<&Arc<dyn Blobstore>>, key: &str, value: &str) {
    if let Some(store) = store {
        let _ = store.put(ctx, key.to_string(), make_value(value)).await;
    }
}

#[fbinit::test]
async fn fetch_blob_missing_all(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (bids, _underlying_stores, stores) = make_empty_stores(3);
    let r = fetch_blob(
        &ctx,
        stores.as_ref(),
        "specialk",
        &HashSet::from_iter(bids.into_iter()),
    )
    .await;
    let msg = r.err().and_then(|e| e.source().map(ToString::to_string));
    assert_eq!(
        Some("None of the blobstores to fetch responded".to_string()),
        msg
    );
    Ok(())
}

#[fbinit::test]
async fn heal_blob_missing_all_stores(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (bids, underlying_stores, stores) = make_empty_stores(3);
    let healing_deadline = DateTime::from_rfc3339("2019-07-01T12:00:00.00Z")?;
    let t0 = DateTime::from_rfc3339("2018-11-29T12:00:00.00Z")?;
    let mp = MultiplexId::new(1);

    let op0 = OperationKey::gen();
    let bsize = Some(make_value("specialv").len() as u64);
    let entries = vec![
        BlobstoreSyncQueueEntry::new("specialk".to_string(), bids[0], mp, t0, op0.clone(), bsize),
        BlobstoreSyncQueueEntry::new("specialk".to_string(), bids[1], mp, t0, op0, bsize),
    ];
    let sync_queue = Arc::new(SqlBlobstoreSyncQueue::with_sqlite_in_memory()?);
    let r = heal_blob(
        &ctx,
        sync_queue.as_ref(),
        stores.as_ref(),
        healing_deadline,
        "specialk".to_string(),
        mp,
        &entries,
    )
    .expect("Expected entries to heal")
    .await;
    let msg = r.err().and_then(|e| e.source().map(ToString::to_string));
    assert_eq!(
        Some("None of the blobstores to fetch responded".to_string()),
        msg
    );
    assert_eq!(
        0,
        sync_queue.len(&ctx, mp).await?,
        "Should be nothing on queue as deletion step won't run"
    );
    assert_eq!(
        0,
        underlying_stores.get(&bids[0]).unwrap().len(),
        "Should still be empty as no healing possible"
    );
    assert_eq!(
        0,
        underlying_stores.get(&bids[1]).unwrap().len(),
        "Should still be empty as no healing possible"
    );
    assert_eq!(
        0,
        underlying_stores.get(&bids[2]).unwrap().len(),
        "Should still be empty as no healing possible"
    );
    Ok(())
}

#[fbinit::test]
async fn heal_blob_where_queue_and_stores_match_on_missing(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (bids, underlying_stores, stores) = make_empty_stores(3);
    put_value(&ctx, stores.get(&bids[0]), "specialk", "specialv").await;
    put_value(&ctx, stores.get(&bids[1]), "specialk", "specialv").await;
    put_value(&ctx, stores.get(&bids[2]), "dummyk", "dummyv").await;
    let healing_deadline = DateTime::from_rfc3339("2019-07-01T12:00:00.00Z")?;
    let t0 = DateTime::from_rfc3339("2018-11-29T12:00:00.00Z")?;
    let mp = MultiplexId::new(1);

    let op0 = OperationKey::gen();
    let bsize = Some(make_value("specialv").len() as u64);
    let entries = vec![
        BlobstoreSyncQueueEntry::new("specialk".to_string(), bids[0], mp, t0, op0.clone(), bsize),
        BlobstoreSyncQueueEntry::new("specialk".to_string(), bids[1], mp, t0, op0, bsize),
    ];
    let sync_queue = Arc::new(SqlBlobstoreSyncQueue::with_sqlite_in_memory()?);
    let r = heal_blob(
        &ctx,
        sync_queue.as_ref(),
        stores.as_ref(),
        healing_deadline,
        "specialk".to_string(),
        mp,
        &entries,
    )
    .expect("expecting to delete entries")
    .await;
    assert!(r.is_ok());
    assert_eq!(1, underlying_stores.get(&bids[0]).unwrap().len());
    assert_eq!(1, underlying_stores.get(&bids[1]).unwrap().len());
    assert_eq!(
        2,
        underlying_stores.get(&bids[2]).unwrap().len(),
        "Expected extra entry after heal"
    );
    assert_eq!(
        0,
        sync_queue.len(&ctx, mp).await?,
        "expecting 0 entries to write to queue for reheal as we just healed the last one"
    );
    Ok(())
}

#[fbinit::test]
async fn fetch_blob_missing_none(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (bids, _underlying_stores, stores) = make_empty_stores(3);
    put_value(&ctx, stores.get(&bids[0]), "specialk", "specialv").await;
    put_value(&ctx, stores.get(&bids[1]), "specialk", "specialv").await;
    put_value(&ctx, stores.get(&bids[2]), "specialk", "specialv").await;
    let r = fetch_blob(
        &ctx,
        stores.as_ref(),
        "specialk",
        &HashSet::from_iter(bids.into_iter()),
    )
    .await;
    let foundv = r.ok().unwrap().blob;
    assert_eq!(make_value("specialv"), foundv);
    Ok(())
}

#[fbinit::test]
async fn test_heal_blob_entry_too_recent(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (bids, underlying_stores, stores) = make_empty_stores(3);
    let healing_deadline = DateTime::from_rfc3339("2019-07-01T12:00:00.00Z")?;
    let t0 = DateTime::from_rfc3339("2019-07-01T11:59:59.00Z")?;
    // too recent,  its after the healing deadline
    let t1 = DateTime::from_rfc3339("2019-07-01T12:00:35.00Z")?;
    let mp = MultiplexId::new(1);

    let op0 = OperationKey::gen();
    let bsize = Some(make_value("specialv").len() as u64);
    let entries = vec![
        BlobstoreSyncQueueEntry::new("specialk".to_string(), bids[0], mp, t0, op0.clone(), bsize),
        BlobstoreSyncQueueEntry::new("specialk".to_string(), bids[1], mp, t1, op0.clone(), bsize),
        BlobstoreSyncQueueEntry::new("specialk".to_string(), bids[2], mp, t0, op0, bsize),
    ];
    let sync_queue = Arc::new(SqlBlobstoreSyncQueue::with_sqlite_in_memory()?);
    let r = heal_blob(
        &ctx,
        sync_queue.as_ref(),
        stores.as_ref(),
        healing_deadline,
        "specialk".to_string(),
        mp,
        &entries,
    );
    assert!(r.is_none(), "expecting that no entries processed");
    assert_eq!(0, sync_queue.len(&ctx, mp).await?);
    assert_eq!(0, underlying_stores.get(&bids[0]).unwrap().len());
    assert_eq!(0, underlying_stores.get(&bids[1]).unwrap().len());
    assert_eq!(0, underlying_stores.get(&bids[2]).unwrap().len());
    Ok(())
}

#[fbinit::test]
async fn heal_blob_missing_none(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (bids, underlying_stores, stores) = make_empty_stores(3);
    put_value(&ctx, stores.get(&bids[0]), "specialk", "specialv").await;
    put_value(&ctx, stores.get(&bids[1]), "specialk", "specialv").await;
    put_value(&ctx, stores.get(&bids[2]), "specialk", "specialv").await;
    let healing_deadline = DateTime::from_rfc3339("2019-07-01T12:00:00.00Z")?;
    let t0 = DateTime::from_rfc3339("2018-11-29T12:00:00.00Z")?;
    let mp = MultiplexId::new(1);

    let op0 = OperationKey::gen();
    let bsize = Some(make_value("specialv").len() as u64);
    let entries = vec![
        BlobstoreSyncQueueEntry::new("specialk".to_string(), bids[0], mp, t0, op0.clone(), bsize),
        BlobstoreSyncQueueEntry::new("specialk".to_string(), bids[1], mp, t0, op0.clone(), bsize),
        BlobstoreSyncQueueEntry::new("specialk".to_string(), bids[2], mp, t0, op0, bsize),
    ];
    let sync_queue = Arc::new(SqlBlobstoreSyncQueue::with_sqlite_in_memory()?);
    let r = heal_blob(
        &ctx,
        sync_queue.as_ref(),
        stores.as_ref(),
        healing_deadline,
        "specialk".to_string(),
        mp,
        &entries,
    )
    .expect("expecting to delete entries")
    .await;
    assert!(r.is_ok());
    assert_eq!(0, sync_queue.len(&ctx, mp).await?);
    assert_eq!(1, underlying_stores.get(&bids[0]).unwrap().len());
    assert_eq!(1, underlying_stores.get(&bids[1]).unwrap().len());
    assert_eq!(1, underlying_stores.get(&bids[2]).unwrap().len());
    Ok(())
}

#[fbinit::test]
async fn test_heal_blob_only_unknown_queue_entry(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (bids, underlying_stores, stores) = make_empty_stores(2);
    let (bids_from_different_config, _, _) = make_empty_stores(5);
    put_value(&ctx, stores.get(&bids[0]), "specialk", "specialv").await;
    let healing_deadline = DateTime::from_rfc3339("2019-07-01T12:00:00.00Z")?;
    let t0 = DateTime::from_rfc3339("2018-11-29T12:00:00.00Z")?;
    let mp = MultiplexId::new(1);

    let op0 = OperationKey::gen();
    let bsize = Some(make_value("specialv").len() as u64);
    let entries = vec![BlobstoreSyncQueueEntry::new(
        "specialk".to_string(),
        bids_from_different_config[4],
        mp,
        t0,
        op0,
        bsize,
    )];
    let sync_queue = Arc::new(SqlBlobstoreSyncQueue::with_sqlite_in_memory()?);
    let r = heal_blob(
        &ctx,
        sync_queue.as_ref(),
        stores.as_ref(),
        healing_deadline,
        "specialk".to_string(),
        mp,
        &entries,
    )
    .expect("expecting to delete entries")
    .await;
    assert!(r.is_ok());
    assert_eq!(
        1,
        sync_queue.len(&ctx, mp).await?,
        "expecting 1 new entries on queue"
    );
    assert_eq!(
        0,
        underlying_stores.get(&bids[1]).unwrap().len(),
        "Expected no change"
    );
    Ok(())
}

#[fbinit::test]
async fn heal_blob_some_unknown_queue_entry(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (bids, underlying_stores, stores) = make_empty_stores(2);
    let (bids_from_different_config, _, _) = make_empty_stores(5);
    put_value(&ctx, stores.get(&bids[0]), "specialk", "specialv").await;
    let healing_deadline = DateTime::from_rfc3339("2019-07-01T12:00:00.00Z")?;
    let t0 = DateTime::from_rfc3339("2018-11-29T12:00:00.00Z")?;
    let mp = MultiplexId::new(1);

    let op0 = OperationKey::gen();
    let bsize = Some(make_value("specialv").len() as u64);
    let entries = vec![
        BlobstoreSyncQueueEntry::new("specialk".to_string(), bids[0], mp, t0, op0.clone(), bsize),
        BlobstoreSyncQueueEntry::new(
            "specialk".to_string(),
            bids_from_different_config[4],
            mp,
            t0,
            op0,
            bsize,
        ),
    ];
    let sync_queue = Arc::new(SqlBlobstoreSyncQueue::with_sqlite_in_memory()?);
    let r = heal_blob(
        &ctx,
        sync_queue.as_ref(),
        stores.as_ref(),
        healing_deadline,
        "specialk".to_string(),
        mp,
        &entries,
    )
    .expect("expecting to delete entries")
    .await;
    assert!(r.is_ok());
    assert_eq!(
        3,
        sync_queue.len(&ctx, mp).await?,
        "expecting 3 new entries on queue, i.e. all sources for known stores, plus the unknown store"
    );
    assert_eq!(
        1,
        underlying_stores.get(&bids[1]).unwrap().len(),
        "Expected put to complete"
    );
    Ok(())
}

#[fbinit::test]
async fn fetch_blob_missing_some(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (bids, _underlying_stores, stores) = make_empty_stores(3);
    put_value(&ctx, stores.get(&bids[0]), "specialk", "specialv").await;
    let r = fetch_blob(
        &ctx,
        stores.as_ref(),
        "specialk",
        &HashSet::from_iter(bids.clone().into_iter()),
    )
    .await;
    let mut fetch_data: FetchData = r.ok().unwrap();
    assert_eq!(make_value("specialv"), fetch_data.blob);
    fetch_data.good_sources.sort();
    assert_eq!(fetch_data.good_sources, &bids[0..1]);
    fetch_data.missing_sources.sort();
    assert_eq!(fetch_data.missing_sources, &bids[1..3]);
    Ok(())
}

#[fbinit::test]
async fn heal_blob_where_queue_and_stores_mismatch_on_missing(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (bids, underlying_stores, stores) = make_empty_stores(3);
    put_value(&ctx, stores.get(&bids[0]), "specialk", "specialv").await;
    put_value(&ctx, stores.get(&bids[1]), "specialk", "specialv").await;
    put_value(&ctx, stores.get(&bids[2]), "dummyk", "dummyv").await;
    let healing_deadline = DateTime::from_rfc3339("2019-07-01T12:00:00.00Z")?;
    let t0 = DateTime::from_rfc3339("2018-11-29T12:00:00.00Z")?;
    let mp = MultiplexId::new(1);

    let op0 = OperationKey::gen();
    let bsize = Some(make_value("specialv").len() as u64);
    let entries = vec![
        BlobstoreSyncQueueEntry::new("specialk".to_string(), bids[0], mp, t0, op0.clone(), bsize),
        BlobstoreSyncQueueEntry::new("specialk".to_string(), bids[2], mp, t0, op0, bsize),
    ];
    let sync_queue = Arc::new(SqlBlobstoreSyncQueue::with_sqlite_in_memory()?);
    let r = heal_blob(
        &ctx,
        sync_queue.as_ref(),
        stores.as_ref(),
        healing_deadline,
        "specialk".to_string(),
        mp,
        &entries,
    )
    .expect("expecting to delete entries")
    .await;
    assert!(r.is_ok());
    assert_eq!(1, underlying_stores.get(&bids[0]).unwrap().len());
    assert_eq!(
        1,
        underlying_stores.get(&bids[1]).unwrap().len(),
        "Expected same entry after heal despite bad queue"
    );
    assert_eq!(
        2,
        underlying_stores.get(&bids[2]).unwrap().len(),
        "Expected extra entry after heal"
    );
    assert_eq!(
        0,
        sync_queue.len(&ctx, mp).await?,
        "expecting 0 entries to write to queue for reheal as all heal puts succeeded"
    );
    Ok(())
}

#[fbinit::test]
async fn heal_blob_where_store_and_queue_match_all_put_fails(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (bids, underlying_stores, stores) = make_empty_stores(3);
    put_value(&ctx, stores.get(&bids[0]), "specialk", "specialv").await;
    put_value(&ctx, stores.get(&bids[1]), "specialk", "specialv").await;
    put_value(&ctx, stores.get(&bids[2]), "dummyk", "dummyv").await;
    let healing_deadline = DateTime::from_rfc3339("2019-07-01T12:00:00.00Z")?;
    let t0 = DateTime::from_rfc3339("2018-11-29T12:00:00.00Z")?;
    let mp = MultiplexId::new(1);

    let op0 = OperationKey::gen();
    let bsize = Some(make_value("specialv").len() as u64);
    let entries = vec![
        BlobstoreSyncQueueEntry::new("specialk".to_string(), bids[0], mp, t0, op0.clone(), bsize),
        BlobstoreSyncQueueEntry::new("specialk".to_string(), bids[1], mp, t0, op0, bsize),
    ];
    underlying_stores.get(&bids[2]).unwrap().fail_puts();
    let sync_queue = Arc::new(SqlBlobstoreSyncQueue::with_sqlite_in_memory()?);
    let r = heal_blob(
        &ctx,
        sync_queue.as_ref(),
        stores.as_ref(),
        healing_deadline,
        "specialk".to_string(),
        mp,
        &entries,
    )
    .expect("expecting to delete entries")
    .await;
    assert!(r.is_ok());
    assert_eq!(1, underlying_stores.get(&bids[0]).unwrap().len());
    assert_eq!(
        1,
        underlying_stores.get(&bids[0]).unwrap().len(),
        "Expected same entry after heal e"
    );
    assert_eq!(
        1,
        underlying_stores.get(&bids[1]).unwrap().len(),
        "Expected same entry after heal"
    );
    assert_eq!(
        1,
        underlying_stores.get(&bids[2]).unwrap().len(),
        "Expected same entry after heal due to put failure"
    );
    assert_eq!(
        2,
        sync_queue.len(&ctx, mp).await?,
        "expecting 2 known good entries to write to queue for reheal as there was a put failure"
    );
    Ok(())
}

#[fbinit::test]
async fn heal_blob_where_store_and_queue_mismatch_some_put_fails(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (bids, underlying_stores, stores) = make_empty_stores(3);
    put_value(&ctx, stores.get(&bids[0]), "specialk", "specialv").await;
    put_value(&ctx, stores.get(&bids[1]), "dummyk", "dummyk").await;
    put_value(&ctx, stores.get(&bids[2]), "dummyk", "dummyv").await;
    let healing_deadline = DateTime::from_rfc3339("2019-07-01T12:00:00.00Z")?;
    let t0 = DateTime::from_rfc3339("2018-11-29T12:00:00.00Z")?;
    let mp = MultiplexId::new(1);

    let op0 = OperationKey::gen();
    let bsize = Some(make_value("specialv").len() as u64);
    let entries = vec![
        BlobstoreSyncQueueEntry::new("specialk".to_string(), bids[0], mp, t0, op0.clone(), bsize),
        BlobstoreSyncQueueEntry::new("specialk".to_string(), bids[1], mp, t0, op0, bsize),
    ];
    underlying_stores.get(&bids[1]).unwrap().fail_puts();
    let sync_queue = Arc::new(SqlBlobstoreSyncQueue::with_sqlite_in_memory()?);
    let r = heal_blob(
        &ctx,
        sync_queue.as_ref(),
        stores.as_ref(),
        healing_deadline,
        "specialk".to_string(),
        mp,
        &entries,
    )
    .expect("Expecting entries to heal")
    .await;
    assert!(r.is_ok());
    assert_eq!(1, underlying_stores.get(&bids[0]).unwrap().len());
    assert_eq!(
        1,
        underlying_stores.get(&bids[0]).unwrap().len(),
        "Expected same entry after heal e"
    );
    assert_eq!(
        1,
        underlying_stores.get(&bids[1]).unwrap().len(),
        "Expected same after heal as put fail prevents heal"
    );
    assert_eq!(
        2,
        underlying_stores.get(&bids[2]).unwrap().len(),
        "Expected extra entry after heal"
    );
    assert_eq!(
        2,
        sync_queue.len(&ctx, mp).await?,
        "expecting 2 known good entries to write to queue for reheal as there was a put failure"
    );
    Ok(())
}

#[fbinit::test]
async fn healer_heal_with_failing_blobstore(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (bids, underlying_stores, stores) = make_empty_stores(2);
    put_value(&ctx, stores.get(&bids[0]), "specialk", "specialv").await;
    underlying_stores.get(&bids[1]).unwrap().fail_puts();

    let t0 = DateTime::from_rfc3339("2018-11-29T12:00:00.00Z")?;
    let mp = MultiplexId::new(1);

    // Insert one entry in the queue for the blobstore that inserted successfully
    let sync_queue = Arc::new(SqlBlobstoreSyncQueue::with_sqlite_in_memory()?);
    let op0 = OperationKey::gen();
    let bsize = Some(make_value("specialv").len() as u64);
    let entries = vec![BlobstoreSyncQueueEntry::new(
        "specialk".to_string(),
        bids[0],
        mp,
        t0,
        op0,
        bsize,
    )];
    sync_queue.add_many(&ctx, entries).await?;

    let healer = Healer::new(
        1000,
        BufferedParams {
            buffer_size: 10,
            weight_limit: 1_000_000_000,
        },
        sync_queue.clone(),
        stores,
        mp,
        None,
        false,
    );

    healer.heal(&ctx, DateTime::now()).await?;

    // Insert to the second blobstore failed, there should be an entry in the queue
    assert_eq!(
        1,
        sync_queue.len(&ctx, mp).await?,
        "expecting an entry that should be rehealed"
    );

    // Now blobstore is "fixed", run the heal again, queue should be empty, second blobstore
    // should have an entry.
    underlying_stores.get(&bids[1]).unwrap().unfail_puts();

    healer.heal(&ctx, DateTime::now()).await?;
    assert_eq!(
        0,
        sync_queue.len(&ctx, mp).await?,
        "expecting everything to be healed"
    );
    assert_eq!(
        underlying_stores
            .get(&bids[1])
            .unwrap()
            .get(&ctx, "specialk")
            .await?,
        Some(BlobstoreGetData::from_bytes(Bytes::from("specialv"))),
    );

    Ok(())
}

#[fbinit::test]
async fn healer_heal_with_default_multiplex_id(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (bids, underlying_stores, stores) = make_empty_stores(2);
    let t0 = DateTime::from_rfc3339("2018-11-29T12:00:00.00Z")?;
    let mp = MultiplexId::new(1);
    let old_mp = MultiplexId::new(-1);

    put_value(&ctx, stores.get(&bids[0]), "specialk", "specialv").await;

    let op0 = OperationKey::gen();
    let op1 = OperationKey::gen();
    let bsize = Some(make_value("specialv").len() as u64);
    let entries = vec![
        BlobstoreSyncQueueEntry::new("specialk".to_string(), bids[0], mp, t0, op0, bsize),
        BlobstoreSyncQueueEntry::new("specialk_mp".to_string(), bids[1], old_mp, t0, op1, bsize),
    ];

    let sync_queue = Arc::new(SqlBlobstoreSyncQueue::with_sqlite_in_memory()?);
    sync_queue.add_many(&ctx, entries).await?;

    // We aren't healing blobs for old_mp, so expect to only have 1 blob in each
    // blobstore at the end of the test.
    let healer = Healer::new(
        1000,
        BufferedParams {
            buffer_size: 10,
            weight_limit: 1_000_000_000,
        },
        sync_queue.clone(),
        stores,
        mp,
        None,
        false,
    );
    healer.heal(&ctx, DateTime::now()).await?;

    assert_eq!(0, sync_queue.len(&ctx, mp).await?);
    assert_eq!(1, sync_queue.len(&ctx, old_mp).await?);

    assert_eq!(1, underlying_stores.get(&bids[0]).unwrap().len());
    assert_eq!(1, underlying_stores.get(&bids[1]).unwrap().len());

    Ok(())
}

#[fbinit::test]
async fn healer_heal_complete_batch(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (bids, _underlying_stores, stores) = make_empty_stores(2);
    let t0 = DateTime::from_rfc3339("2018-11-29T12:00:00.00Z")?;
    let mp = MultiplexId::new(1);

    put_value(&ctx, stores.get(&bids[0]), "specialk", "specialv").await;
    put_value(&ctx, stores.get(&bids[1]), "specialk", "specialv").await;

    let op0 = OperationKey::gen();
    let op1 = OperationKey::gen();
    let bsize = Some(make_value("specialv").len() as u64);
    let entries = vec![
        BlobstoreSyncQueueEntry::new("specialk".to_string(), bids[0], mp, t0, op0.clone(), bsize),
        BlobstoreSyncQueueEntry::new("specialk".to_string(), bids[1], mp, t0, op0, bsize),
        BlobstoreSyncQueueEntry::new("specialk".to_string(), bids[0], mp, t0, op1.clone(), bsize),
        BlobstoreSyncQueueEntry::new("specialk".to_string(), bids[1], mp, t0, op1, bsize),
    ];

    let sync_queue = Arc::new(SqlBlobstoreSyncQueue::with_sqlite_in_memory()?);
    sync_queue.add_many(&ctx, entries).await?;

    let healer = Healer::new(
        2,
        BufferedParams {
            buffer_size: 10,
            weight_limit: 1_000_000_000,
        },
        sync_queue,
        stores,
        mp,
        None,
        false,
    );
    let (complete_batch, _) = healer.heal(&ctx, DateTime::now()).await?;
    assert!(complete_batch);
    Ok(())
}

#[fbinit::test]
async fn healer_heal_incomplete_batch(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (bids, _underlying_stores, stores) = make_empty_stores(2);
    let t0 = DateTime::from_rfc3339("2018-11-29T12:00:00.00Z")?;
    let mp = MultiplexId::new(1);

    put_value(&ctx, stores.get(&bids[0]), "specialk", "specialv").await;
    put_value(&ctx, stores.get(&bids[1]), "specialk", "specialv").await;

    let op0 = OperationKey::gen();
    let op1 = OperationKey::gen();
    let bsize = Some(make_value("specialv").len() as u64);
    let entries = vec![
        BlobstoreSyncQueueEntry::new("specialk".to_string(), bids[0], mp, t0, op0.clone(), bsize),
        BlobstoreSyncQueueEntry::new("specialk".to_string(), bids[1], mp, t0, op0, bsize),
        BlobstoreSyncQueueEntry::new("specialk".to_string(), bids[0], mp, t0, op1.clone(), bsize),
        BlobstoreSyncQueueEntry::new("specialk".to_string(), bids[1], mp, t0, op1, bsize),
    ];

    let sync_queue = Arc::new(SqlBlobstoreSyncQueue::with_sqlite_in_memory()?);
    sync_queue.add_many(&ctx, entries).await?;

    let healer = Healer::new(
        20,
        BufferedParams {
            buffer_size: 10,
            weight_limit: 1_000_000_000,
        },
        sync_queue,
        stores,
        mp,
        None,
        false,
    );
    let (complete_batch, _) = healer.heal(&ctx, DateTime::now()).await?;
    assert!(!complete_batch);
    Ok(())
}
