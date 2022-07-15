/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Result;
use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore::BlobstorePutOps;
use blobstore_sync_queue::BlobstoreWal;
use blobstore_sync_queue::OperationKey;
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
use scuba_ext::MononokeScubaSampleBuilder;
use sql_construct::SqlConstruct;
use std::fmt::Debug;
use std::future::Future;
use std::panic;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use crate::MultiplexTimeout;
use crate::Scuba;
use crate::WalMultiplexedBlobstore;

#[fbinit::test]
async fn test_quorum_is_valid(_fb: FacebookInit) -> Result<()> {
    let scuba = Scuba::new(MononokeScubaSampleBuilder::with_discard(), 1u64)?;
    let wal = Arc::new(SqlBlobstoreWal::with_sqlite_in_memory()?);

    // Check the quorum cannot be zero
    {
        // no main-stores, no write-mostly
        assert!(setup_multiplex(0, 0, None).is_err());
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
            None,
            scuba.clone(),
        );

        assert!(result.is_err());
    }

    // Check creating multiplex succeeds with the same amount of stores as the quorum
    {
        assert!(setup_multiplex(3, 3, None).is_ok());
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
    let ctx = CoreContext::test_mock(fb);
    let (tickable_queue, tickable_blobstores, multiplex) = setup_multiplex(3, 2, None)?;

    let v = make_value("v");
    let k = "k";

    let mut put_fut = multiplex.put(&ctx, k.to_owned(), v).map_err(|_| ()).boxed();
    assert_pending(&mut put_fut).await;

    // wal queue write fails
    tickable_queue.tick(Some("wal queue failed"));

    // multiplex put should fail
    assert!(put_fut.await.is_err());

    // check there is no blob in the storage
    {
        let mut get_fut = multiplex.get(&ctx, k).map_err(|_| ()).boxed();
        assert_pending(&mut get_fut).await;

        // blobstore gets succeed
        for (_id, store) in tickable_blobstores.iter() {
            store.tick(None);
        }
        validate_blob(get_fut.await, Ok(None));
    }

    Ok(())
}

#[fbinit::test]
async fn test_put_fails(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (tickable_queue, tickable_blobstores, multiplex) = setup_multiplex(3, 2, None)?;

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

        // No `put` succeeded, there is no blob in the storage
        {
            let mut get_fut = multiplex.get(&ctx, k).map_err(|_| ()).boxed();
            assert_pending(&mut get_fut).await;

            // blobstore gets succeed
            for (_id, store) in tickable_blobstores.iter() {
                store.tick(None);
            }
            validate_blob(get_fut.await, Ok(None));
        }
    }

    // Second blobstore put succeeded but no quorum was achieved, multiplex put fails: [x] [ ] [x]
    {
        let v = make_value("v1");
        let k = "k1";

        let mut put_fut = multiplex
            .put(&ctx, k.to_owned(), v.clone())
            .map_err(|_| ())
            .boxed();
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

        // The blob was written to the second blobstore even if the multiplex put failed.
        // That means `get` result is undefined: it can either return `None` or `Some`
        // depending on which blobstores' put returned first.
        //
        // This is fine because if multiplex `put` failed, multiplex blobstore doesn't provide
        // any guarantees whether the blob is present in the storage or not.
        // There is only guarantee: if `put` succeeded, the blob will always be present,
        // i.e. `get` will always return `Some` (if it didn't fail for some other reason).

        // 1st and 3rd blobstore returned `None`, before 2nd blobstore returned anything
        // there is a read quorum on `None`, so multiplex `get` should also return `None`
        {
            let mut get_fut = multiplex.get(&ctx, k).map_err(|_| ()).boxed();
            assert_pending(&mut get_fut).await;

            // first and third blobstores don't have the blob
            tickable_blobstores[0].1.tick(None);
            tickable_blobstores[2].1.tick(None);

            // the result is ready
            validate_blob(get_fut.await, Ok(None));
            tickable_blobstores[1].1.drain(1);
        }

        // 2nd blobstore returned before the read quorum on `None` was achieved, so
        // multiplex `get` should also return `Some`
        {
            let mut get_fut = multiplex.get(&ctx, k).map_err(|_| ()).boxed();
            assert_pending(&mut get_fut).await;

            // first blobstore doesn't have the blob
            tickable_blobstores[0].1.tick(None);
            // second blobstore has
            tickable_blobstores[1].1.tick(None);

            // the result is ready
            validate_blob(get_fut.await, Ok(Some(&v)));
            tickable_blobstores[2].1.drain(1);
        }
    }

    Ok(())
}

#[fbinit::test]
async fn test_put_succeeds(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let quorum = 2;
    let (tickable_queue, tickable_blobstores, multiplex) = setup_multiplex(3, quorum, None)?;

    // Quorum puts succeed, the multiplex put succeeds: [ ] [x] [ ]
    // Should wait for the third put to complete.
    {
        let v = make_value("v2");
        let k = "k2";

        let mut put_fut = multiplex
            .put(&ctx, k.to_owned(), v.clone())
            .map_err(|_| ())
            .boxed();
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

        // check we can read the blob from the 1st store
        {
            let mut get_fut = multiplex.get(&ctx, k).map_err(|_| ()).boxed();
            assert_pending(&mut get_fut).await;

            // first blobstore returns the blob
            tickable_blobstores[0].1.tick(None);
            validate_blob(get_fut.await, Ok(Some(&v)));

            // drain the tickables of the pending requests, as they won't be claimed
            for (_id, store) in &tickable_blobstores[1..3] {
                store.drain(1);
            }
        }

        // check we can read the blob from the 3rd store
        {
            let mut get_fut = multiplex.get(&ctx, k).map_err(|_| ()).boxed();
            assert_pending(&mut get_fut).await;

            // first blobstore fails
            tickable_blobstores[0].1.tick(Some("bs0 failed"));
            assert_pending(&mut get_fut).await;

            // second blobstore doesn't have the blob
            tickable_blobstores[1].1.tick(None);
            assert_pending(&mut get_fut).await;

            // third blobstore succeeds
            tickable_blobstores[2].1.tick(None);
            validate_blob(get_fut.await, Ok(Some(&v)));
        }
    }

    // All puts succeed, the multiplex put succeeds: [ ] [ ] [ ]
    // Should not wait for the third put to complete.
    {
        let v = make_value("v3");
        let k = "k3";

        let mut put_fut = multiplex
            .put(&ctx, k.to_owned(), v.clone())
            .map_err(|_| ())
            .boxed();
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

        // check we can read the blob
        {
            let mut get_fut = multiplex.get(&ctx, k).map_err(|_| ()).boxed();
            assert_pending(&mut get_fut).await;

            // blobstore gets succeed
            for (_id, store) in tickable_blobstores.iter() {
                store.tick(None);
            }
            validate_blob(get_fut.await, Ok(Some(&v)));
        }
    }

    Ok(())
}

#[fbinit::test]
async fn test_get_on_missing(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (_tickable_queue, tickable_blobstores, multiplex) = setup_multiplex(3, 2, None)?;

    // No blobstores have the key
    let k = "k1";

    // all gets succeed, but multiplexed returns `None`
    {
        let mut get_fut = multiplex.get(&ctx, k).map_err(|_| ()).boxed();
        assert_pending(&mut get_fut).await;

        tickable_blobstores[0].1.tick(None);
        assert_pending(&mut get_fut).await;
        tickable_blobstores[1].1.tick(None);

        // the read-quorum on `None` achieved, multiplexed get returns `None`
        validate_blob(get_fut.await, Ok(None));
        tickable_blobstores[2].1.drain(1);
    }

    // two gets succeed, but multiplexed returns `None`
    {
        let mut get_fut = multiplex.get(&ctx, k).map_err(|_| ()).boxed();
        assert_pending(&mut get_fut).await;

        tickable_blobstores[0].1.tick(None);
        tickable_blobstores[1].1.tick(Some("bs1 failed"));
        // muliplexed get waits on the third
        assert_pending(&mut get_fut).await;
        tickable_blobstores[2].1.tick(None);

        // the read-quorum on `None` achieved, multiplexed get returns `None`
        validate_blob(get_fut.await, Ok(None));
    }

    // two gets fail, multiplexed get fails, because no read quorum
    {
        let mut get_fut = multiplex.get(&ctx, k).map_err(|_| ()).boxed();
        assert_pending(&mut get_fut).await;

        tickable_blobstores[0].1.tick(Some("bs0 failed"));
        tickable_blobstores[1].1.tick(Some("bs1 failed"));
        // muliplexed get waits on the third, which returns `None`
        assert_pending(&mut get_fut).await;
        tickable_blobstores[2].1.tick(None);

        // no read-quorum, multiplexed get fails
        validate_blob(get_fut.await, Err(()));
    }

    Ok(())
}

#[fbinit::test]
async fn test_get_on_existing(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (tickable_queue, tickable_blobstores, multiplex) = setup_multiplex(3, 2, None)?;

    // Two blobstores have the key, one failed to write: [ ] [x] [ ]

    let v = make_value("v1");
    let k = "k1";

    let mut put_fut = multiplex
        .put(&ctx, k.to_owned(), v.clone())
        .map_err(|_| ())
        .boxed();
    assert_pending(&mut put_fut).await;

    // wal queue write succeeds
    tickable_queue.tick(None);
    assert_pending(&mut put_fut).await;

    tickable_blobstores[0].1.tick(None);
    tickable_blobstores[1].1.tick(Some("bs1 failed"));
    tickable_blobstores[2].1.tick(None);

    // multiplexed put succeeds: write quorum achieved
    assert!(put_fut.await.is_ok());

    // all gets succeed, but multiplexed returns on the first successful `Some`
    {
        let mut get_fut = multiplex.get(&ctx, k).map_err(|_| ()).boxed();
        assert_pending(&mut get_fut).await;

        // first blobstore returns the blob
        tickable_blobstores[0].1.tick(None);
        validate_blob(get_fut.await, Ok(Some(&v)));

        // drain the tickables of the pending requests, as they won't be claimed
        for (_id, store) in &tickable_blobstores[1..3] {
            store.drain(1);
        }
    }

    // first get fails, but multiplexed returns on the third get
    {
        let mut get_fut = multiplex.get(&ctx, k).map_err(|_| ()).boxed();
        assert_pending(&mut get_fut).await;

        // first blobstore get fails
        tickable_blobstores[0].1.tick(Some("bs1 failed!"));
        assert_pending(&mut get_fut).await;

        // second blobstore get returns None
        tickable_blobstores[1].1.tick(None);
        assert_pending(&mut get_fut).await;

        // third blobstore get returns Some
        tickable_blobstores[2].1.tick(None);
        validate_blob(get_fut.await, Ok(Some(&v)));
    }

    // 2 first gets fail, but multiplexed returns `Some` on the third get
    {
        let mut get_fut = multiplex.get(&ctx, k).map_err(|_| ()).boxed();
        assert_pending(&mut get_fut).await;

        // first blobstore get fails
        tickable_blobstores[0].1.tick(Some("bs1 failed!"));
        assert_pending(&mut get_fut).await;

        // second blobstore get fails
        tickable_blobstores[1].1.tick(Some("bs2 failed!"));
        assert_pending(&mut get_fut).await;

        // third blobstore get returns Some
        tickable_blobstores[2].1.tick(None);
        validate_blob(get_fut.await, Ok(Some(&v)));
    }

    // all blobstores that have the blob fail, multiplexed get fail:
    // no read quorum on `None` was achieved
    {
        let mut get_fut = multiplex.get(&ctx, k).map_err(|_| ()).boxed();
        assert_pending(&mut get_fut).await;

        // first and third blobstore gets fail
        tickable_blobstores[0].1.tick(Some("bs1 failed!"));
        tickable_blobstores[2].1.tick(Some("bs3 failed!"));
        assert_pending(&mut get_fut).await;

        // second blobstore get returns None
        tickable_blobstores[1].1.tick(None);
        validate_blob(get_fut.await, Err(()));
    }

    // all blobstores gets fail, multiplexed get fail
    {
        let mut get_fut = multiplex.get(&ctx, k).map_err(|_| ()).boxed();
        assert_pending(&mut get_fut).await;

        for (id, store) in tickable_blobstores {
            store.tick(Some(format!("bs{} failed!", id).as_str()));
        }

        validate_blob(get_fut.await, Err(()));
    }

    Ok(())
}

#[fbinit::test]
async fn test_is_present_missing(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (_tickable_queue, tickable_blobstores, multiplex) = setup_multiplex(3, 2, None)?;

    // No blobstores have the key
    let k = "k1";

    // all `is_present` succeed, multiplexed returns `Absent`
    {
        let mut fut = multiplex.is_present(&ctx, k).map_err(|_| ()).boxed();
        assert_pending(&mut fut).await;

        tickable_blobstores[0].1.tick(None);
        assert_pending(&mut fut).await;
        tickable_blobstores[1].1.tick(None);

        // the read-quorum on `None` achieved, multiplexed returns `Absent`
        assert_is_present_ok(fut.await, BlobstoreIsPresent::Absent);
        tickable_blobstores[2].1.drain(1);
    }

    // two `is_present`s succeed, multiplexed returns `Absent`
    {
        let mut fut = multiplex.is_present(&ctx, k).map_err(|_| ()).boxed();
        assert_pending(&mut fut).await;

        tickable_blobstores[0].1.tick(None);
        tickable_blobstores[1].1.tick(Some("bs1 failed"));
        // muliplexed is_present waits on the third
        assert_pending(&mut fut).await;
        tickable_blobstores[2].1.tick(None);

        // the read-quorum achieved, multiplexed returns `Absent`
        assert_is_present_ok(fut.await, BlobstoreIsPresent::Absent);
    }

    // two `is_present`s fail, multiplexed returns `ProbablyNotPresent`
    {
        let mut fut = multiplex.is_present(&ctx, k).map_err(|_| ()).boxed();
        assert_pending(&mut fut).await;

        tickable_blobstores[0].1.tick(Some("bs0 failed"));
        tickable_blobstores[1].1.tick(None);
        // muliplexed is_present waits on the third
        assert_pending(&mut fut).await;
        tickable_blobstores[2].1.tick(Some("bs2 failed"));

        assert_is_present_ok(
            fut.await,
            BlobstoreIsPresent::ProbablyNotPresent(anyhow!("some failed!")),
        );
    }

    // all `is_present`s fail, multiplexed fails
    {
        let mut fut = multiplex.is_present(&ctx, k).map_err(|_| ()).boxed();
        assert_pending(&mut fut).await;

        for (id, store) in tickable_blobstores {
            store.tick(Some(format!("bs{} failed!", id).as_str()));
        }
        assert!(fut.await.is_err());
    }

    Ok(())
}

#[fbinit::test]
async fn test_is_present_existing(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (tickable_queue, tickable_blobstores, multiplex) = setup_multiplex(3, 2, None)?;

    // Two blobstores have the key, one failed to write: [ ] [x] [ ]
    {
        let v = make_value("v1");
        let k = "k1";

        let mut put_fut = multiplex
            .put(&ctx, k.to_owned(), v.clone())
            .map_err(|_| ())
            .boxed();
        assert_pending(&mut put_fut).await;

        // wal queue write succeeds
        tickable_queue.tick(None);
        assert_pending(&mut put_fut).await;

        tickable_blobstores[0].1.tick(None);
        tickable_blobstores[1].1.tick(Some("bs1 failed"));
        tickable_blobstores[2].1.tick(None);

        // multiplexed put succeeds: write quorum achieved
        assert!(put_fut.await.is_ok());

        // first `is_present` succeed with `Present`, multiplexed returns `Present`
        {
            let mut fut = multiplex.is_present(&ctx, k).map_err(|_| ()).boxed();
            assert_pending(&mut fut).await;

            tickable_blobstores[0].1.tick(None);
            assert_is_present_ok(fut.await, BlobstoreIsPresent::Present);

            for (_id, store) in &tickable_blobstores[1..] {
                store.drain(1);
            }
        }

        // first `is_present` fails, second succeed with `Absent`, third returns `Present`
        // multiplexed returns `Present`
        {
            let mut fut = multiplex.is_present(&ctx, k).map_err(|_| ()).boxed();
            assert_pending(&mut fut).await;

            tickable_blobstores[0].1.tick(Some("bs0 failed"));
            tickable_blobstores[1].1.tick(None);
            assert_pending(&mut fut).await;

            tickable_blobstores[2].1.tick(None);
            assert_is_present_ok(fut.await, BlobstoreIsPresent::Present);
        }
    }

    // Two blobstores failed to write, one succeeded: [x] [ ] [x]
    {
        let v = make_value("v2");
        let k = "k2";

        let mut put_fut = multiplex
            .put(&ctx, k.to_owned(), v.clone())
            .map_err(|_| ())
            .boxed();
        assert_pending(&mut put_fut).await;

        // wal queue write succeeds
        tickable_queue.tick(None);
        assert_pending(&mut put_fut).await;

        tickable_blobstores[0].1.tick(Some("bs0 failed"));
        tickable_blobstores[1].1.tick(None);
        tickable_blobstores[2].1.tick(Some("bs2 failed"));

        // multiplexed put failed: no write quorum
        assert!(put_fut.await.is_err());

        // the first `is_present` to succeed returns `Present`, multiplexed returns `Present`
        {
            let mut fut = multiplex.is_present(&ctx, k).map_err(|_| ()).boxed();
            assert_pending(&mut fut).await;

            tickable_blobstores[1].1.tick(None);
            assert_is_present_ok(fut.await, BlobstoreIsPresent::Present);

            tickable_blobstores[0].1.drain(1);
            tickable_blobstores[2].1.drain(1);
        }

        // if the first two `is_present` calls return `Absent`, multiplexed returns `Absent`
        {
            let mut fut = multiplex.is_present(&ctx, k).map_err(|_| ()).boxed();
            assert_pending(&mut fut).await;

            tickable_blobstores[0].1.tick(None);
            tickable_blobstores[2].1.tick(None);

            assert_is_present_ok(fut.await, BlobstoreIsPresent::Absent);
            tickable_blobstores[1].1.drain(1);
        }

        // if one `is_present` returns `Absent`, another 2 fail, multiplexed is unsure
        {
            let mut fut = multiplex.is_present(&ctx, k).map_err(|_| ()).boxed();
            assert_pending(&mut fut).await;

            tickable_blobstores[0].1.tick(None);
            for (id, store) in &tickable_blobstores[1..] {
                store.tick(Some(format!("bs{} failed", id).as_str()));
            }

            assert_is_present_ok(
                fut.await,
                BlobstoreIsPresent::ProbablyNotPresent(anyhow!("some failed!")),
            );
        }
    }

    Ok(())
}

#[fbinit::test]
async fn test_timeout_on_request(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    // Ensure that even if the quorum writes succeeded, the rest of the writes are not
    // being dropped.
    {
        let timeout = MultiplexTimeout {
            // we want writes to succeed
            write: Duration::from_secs(10),
            // and reads to fail because of timeout
            read: Duration::from_millis(5),
        };
        let (tickable_queue, tickable_blobstores, multiplex) =
            setup_multiplex(3, 2, Some(timeout))?;

        let v = make_value("v1");
        let k = "k1";

        let mut put_fut = multiplex
            .put(&ctx, k.to_owned(), v.clone())
            .map_err(|_| ())
            .boxed();
        assert_pending(&mut put_fut).await;

        // wal queue write succeeds
        tickable_queue.tick(None);
        assert_pending(&mut put_fut).await;

        // quorum puts succeed -> the multiplexed put succeeds
        tickable_blobstores[0].1.tick(None);
        tickable_blobstores[1].1.tick(None);
        assert!(put_fut.await.is_ok());

        // Now we'll try to respond to the last put, that haven't completed.
        // It should not panic, as the future is still in flight and still
        // waits for the response.
        tokio::time::sleep(Duration::from_millis(5)).await;
        let result = panic::catch_unwind(|| tickable_blobstores[2].1.tick(None));
        assert!(result.is_ok());

        // We'll try to read with a delayed response from the blobstores.
        // Get should fail.
        let mut fut = multiplex.get(&ctx, k).map_err(|_| ()).boxed();
        assert_pending(&mut fut).await;
        tokio::time::sleep(Duration::from_millis(25)).await;
        assert!(fut.await.is_err());
    }

    // Ensure that after the quorum writes succeeded, the rest of the writes are
    // dropped because of the timeout.
    {
        // set write timeout to a very low value
        let timeout = MultiplexTimeout::new(
            None,                           /* read */
            Some(Duration::from_millis(5)), /* write */
        );
        let (tickable_queue, tickable_blobstores, multiplex) =
            setup_multiplex(3, 2, Some(timeout))?;

        let v = make_value("v2");
        let k = "k2";

        let mut put_fut = multiplex
            .put(&ctx, k.to_owned(), v.clone())
            .map_err(|_| ())
            .boxed();
        assert_pending(&mut put_fut).await;

        // wal queue write succeeds
        tickable_queue.tick(None);
        assert_pending(&mut put_fut).await;

        // quorum puts succeed -> the multiplexed put succeeds
        tickable_blobstores[0].1.tick(None);
        tickable_blobstores[1].1.tick(None);
        assert!(put_fut.await.is_ok());

        // Now we'll delay the response again and respond to the last put,
        // that haven't completed.
        // It *should* panic, as the future was dropped due to the timeout.
        tokio::time::sleep(Duration::from_millis(25)).await;
        let result = panic::catch_unwind(|| tickable_blobstores[2].1.tick(None));
        assert!(result.is_err());
    }

    Ok(())
}

async fn assert_pending<T: Debug>(fut: &mut (impl Future<Output = T> + Unpin)) {
    match PollOnce::new(fut).await {
        Poll::Pending => {}
        state => {
            panic!("future must be pending, received: {:?}", state);
        }
    }
}

fn setup_multiplex(
    num: u64,
    quorum: usize,
    timeout: Option<MultiplexTimeout>,
) -> Result<(
    Arc<Tickable<OperationKey>>,
    Vec<(BlobstoreId, Arc<Tickable<(BlobstoreBytes, u64)>>)>,
    WalMultiplexedBlobstore,
)> {
    let (tickable_queue, wal_queue) = setup_queue();
    let (tickable_blobstores, blobstores) = setup_blobstores(num);
    let scuba = Scuba::new(MononokeScubaSampleBuilder::with_discard(), 1u64)?;
    let multiplex = WalMultiplexedBlobstore::new(
        MultiplexId::new(1),
        wal_queue,
        blobstores,
        vec![],
        quorum,
        timeout,
        scuba,
    )?;

    Ok((tickable_queue, tickable_blobstores, multiplex))
}

type TickableBytes = Tickable<(BlobstoreBytes, u64)>;

fn setup_blobstores(
    num: u64,
) -> (
    Vec<(BlobstoreId, Arc<TickableBytes>)>,
    Vec<(BlobstoreId, Arc<dyn BlobstorePutOps>)>,
) {
    let tickable_blobstores: Vec<_> = (0..num)
        .map(|id| (BlobstoreId::new(id), Arc::new(TickableBytes::new())))
        .collect();
    let blobstores = tickable_blobstores
        .clone()
        .into_iter()
        .map(|(id, store)| (id, store as Arc<dyn BlobstorePutOps>))
        .collect();
    (tickable_blobstores, blobstores)
}

fn setup_queue() -> (Arc<Tickable<OperationKey>>, Arc<dyn BlobstoreWal>) {
    let tickable_queue = Arc::new(Tickable::new());
    let wal_queue = tickable_queue.clone() as Arc<dyn BlobstoreWal>;
    (tickable_queue, wal_queue)
}

fn make_value(value: &str) -> BlobstoreBytes {
    BlobstoreBytes::from_bytes(Bytes::copy_from_slice(value.as_bytes()))
}

fn validate_blob(
    get_data: Result<Option<BlobstoreGetData>, ()>,
    expected: Result<Option<&BlobstoreBytes>, ()>,
) {
    assert_eq!(get_data.is_ok(), expected.is_ok());
    if let Ok(expected) = expected {
        let get_data = get_data.unwrap().map(|data| data.into_bytes());
        assert_eq!(get_data.as_ref(), expected);
    }
}

fn assert_is_present_ok(result: Result<BlobstoreIsPresent, ()>, expected: BlobstoreIsPresent) {
    assert!(result.is_ok());
    match (result.unwrap(), expected) {
        (BlobstoreIsPresent::Absent, BlobstoreIsPresent::Absent)
        | (BlobstoreIsPresent::Present, BlobstoreIsPresent::Present)
        | (BlobstoreIsPresent::ProbablyNotPresent(_), BlobstoreIsPresent::ProbablyNotPresent(_)) => {
        }
        (res, exp) => {
            panic!(
                "`is_present` call must return {:?}, received: {:?}",
                exp, res
            );
        }
    }
}
