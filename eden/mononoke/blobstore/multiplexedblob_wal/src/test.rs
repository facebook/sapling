/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use blobstore::Blobstore;
use blobstore::BlobstorePutOps;
use blobstore_sync_queue::BlobstoreWal;
use blobstore_sync_queue::SqlBlobstoreWal;
use blobstore_test_utils::Tickable;
use bytes::Bytes;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::future::FutureExt;
use futures::future::TryFutureExt;
use futures::task::Context;
use futures::task::Poll;
use metaconfig_types::BlobstoreId;
use metaconfig_types::MultiplexId;
use mononoke_types::BlobstoreBytes;
use sql_construct::SqlConstruct;
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::WalMultiplexedBlobstore;

#[fbinit::test]
async fn test_quorum_is_valid(_fb: FacebookInit) -> Result<()> {
    let wal = Arc::new(SqlBlobstoreWal::with_sqlite_in_memory()?);

    // Check the quorum cannot be zero
    {
        // no main-stores, no write-mostly
        let quorum = 0;
        let result =
            WalMultiplexedBlobstore::new(MultiplexId::new(0), wal.clone(), vec![], vec![], quorum);

        assert!(result.is_err());
    }

    // Check creating multiplex fails if there are no enough main blobstores
    {
        let stores = (0..2)
            .map(|id| {
                (
                    BlobstoreId::new(id),
                    Arc::new(Tickable::new()) as Arc<dyn BlobstorePutOps>,
                )
            })
            .collect();
        // write-mostly don't count into the quorum
        let write_mostly = (2..4)
            .map(|id| {
                (
                    BlobstoreId::new(id),
                    Arc::new(Tickable::new()) as Arc<dyn BlobstorePutOps>,
                )
            })
            .collect();
        let quorum = 3;
        let result = WalMultiplexedBlobstore::new(
            MultiplexId::new(0),
            wal.clone(),
            stores,
            write_mostly,
            quorum,
        );

        assert!(result.is_err());
    }

    // Check creating multiplex succeeds with the same amount of stores as the quorum
    {
        let stores = (0..3)
            .map(|id| {
                (
                    BlobstoreId::new(id),
                    Arc::new(Tickable::new()) as Arc<dyn BlobstorePutOps>,
                )
            })
            .collect();
        // no write-mostly
        let quorum = 3;
        let result = WalMultiplexedBlobstore::new(MultiplexId::new(0), wal, stores, vec![], quorum);

        assert!(result.is_ok());
    }

    Ok(())
}

struct PollOnce<'a, F> {
    future: Pin<&'a mut F>,
}

impl<'a, F: Future + Unpin> PollOnce<'a, F> {
    pub fn new(future: &'a mut F) -> Self {
        Self {
            future: Pin::new(future),
        }
    }
}

impl<'a, F: Future + Unpin> Future for PollOnce<'a, F> {
    type Output = Poll<<F as Future>::Output>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        // This is pin-projection; I uphold the Pin guarantees, so it's fine.
        let this = unsafe { self.get_unchecked_mut() };
        Poll::Ready(this.future.poll_unpin(cx))
    }
}

#[fbinit::test]
async fn test_put_wal_fails(fb: FacebookInit) -> Result<()> {
    let tickable_blobstores: Vec<_> = (0..3)
        .map(|id| (BlobstoreId::new(id), Arc::new(Tickable::new())))
        .collect();
    let blobstores = tickable_blobstores
        .clone()
        .into_iter()
        .map(|(id, store)| (id, store as Arc<dyn BlobstorePutOps>))
        .collect();

    let quorum = 2;

    let tickable_queue = Arc::new(Tickable::new());
    let wal_queue = tickable_queue.clone() as Arc<dyn BlobstoreWal>;
    let multiplex =
        WalMultiplexedBlobstore::new(MultiplexId::new(1), wal_queue, blobstores, vec![], quorum)?;

    let ctx = CoreContext::test_mock(fb);

    let v = make_value("v");
    let k = "k".to_owned();

    let mut put_fut = multiplex.put(&ctx, k, v).map_err(|_| ()).boxed();
    assert_pending(&mut put_fut).await;

    // wal queue write fails
    tickable_queue.tick(Some("wal queue failed"));

    // multiplex put should fail
    assert!(put_fut.await.is_err());

    Ok(())
}

#[fbinit::test]
async fn test_puts(fb: FacebookInit) -> Result<()> {
    let tickable_blobstores: Vec<_> = (0..3)
        .map(|id| (BlobstoreId::new(id), Arc::new(Tickable::new())))
        .collect();
    let blobstores = tickable_blobstores
        .clone()
        .into_iter()
        .map(|(id, store)| (id, store as Arc<dyn BlobstorePutOps>))
        .collect();

    let quorum = 2;

    let tickable_queue = Arc::new(Tickable::new());
    let wal_queue = tickable_queue.clone() as Arc<dyn BlobstoreWal>;
    let multiplex =
        WalMultiplexedBlobstore::new(MultiplexId::new(1), wal_queue, blobstores, vec![], quorum)?;

    let ctx = CoreContext::test_mock(fb);

    // All puts fail, the multiplex put should fail: [x] [x] [x]
    {
        let v = make_value("v0");
        let k = "k0";

        let mut put_fut = multiplex.put(&ctx, k.to_owned(), v).map_err(|_| ()).boxed();
        assert_pending(&mut put_fut).await;

        // wal queue write succeeds
        tickable_queue.tick(None);
        assert_pending(&mut put_fut).await;

        // all blobstores should fail
        for (id, store) in tickable_blobstores.iter() {
            store.tick(Some(format!("all fail: bs{} failed", id).as_str()));
        }

        assert!(put_fut.await.is_err());
    }

    // No quorum puts succeeded, the multiplex put fails: [x] [ ] [x]
    {
        let v = make_value("v1");
        let k = "k1";

        let mut put_fut = multiplex.put(&ctx, k.to_owned(), v).map_err(|_| ()).boxed();
        assert_pending(&mut put_fut).await;

        // wal queue write succeeds
        tickable_queue.tick(None);
        assert_pending(&mut put_fut).await;

        // first blobstore fails
        tickable_blobstores[0].1.tick(Some("all fail: bs0 failed"));
        assert_pending(&mut put_fut).await;
        // second blobstore succeeds
        tickable_blobstores[1].1.tick(None);
        assert_pending(&mut put_fut).await;
        // third blobstore fails
        tickable_blobstores[2].1.tick(Some("all fail: bs2 failed"));

        // the multiplex put fails
        assert!(put_fut.await.is_err());
    }

    // Quorum puts succeed, the multiplex put succeeds: [ ] [x] [ ]
    // Should wait for the third put to complete.
    {
        let v = make_value("v2");
        let k = "k2";

        let mut put_fut = multiplex.put(&ctx, k.to_owned(), v).map_err(|_| ()).boxed();
        assert_pending(&mut put_fut).await;

        // wal queue write succeeds
        tickable_queue.tick(None);
        assert_pending(&mut put_fut).await;

        // first blobstore succeeds
        tickable_blobstores[0].1.tick(None);
        assert_pending(&mut put_fut).await;
        // second blobstore fails
        tickable_blobstores[1].1.tick(Some("all fail: bs1 failed"));
        assert_pending(&mut put_fut).await;
        // third blobstore succeeds
        tickable_blobstores[2].1.tick(None);

        assert!(put_fut.await.is_ok());
    }

    // All puts succeed, the multiplex put succeeds: [ ] [ ] [ ]
    // Should not wait for the third put to complete.
    {
        let v = make_value("v3");
        let k = "k3";

        let mut put_fut = multiplex.put(&ctx, k.to_owned(), v).map_err(|_| ()).boxed();
        assert_pending(&mut put_fut).await;

        // wal queue write succeeds
        tickable_queue.tick(None);
        assert_pending(&mut put_fut).await;

        // first quorum blobstore puts succeed, multiplex doesn't wait for the rest
        // of the puts
        for (_id, store) in &tickable_blobstores[0..quorum] {
            store.tick(None);
        }

        assert!(put_fut.await.is_ok());
    }

    Ok(())
}

async fn assert_pending<T: PartialEq + Debug>(fut: &mut (impl Future<Output = T> + Unpin)) {
    assert_eq!(PollOnce::new(fut).await, Poll::Pending);
}

fn make_value(value: &str) -> BlobstoreBytes {
    BlobstoreBytes::from_bytes(Bytes::copy_from_slice(value.as_bytes()))
}
