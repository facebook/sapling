/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobstore::{Blobstore, BlobstoreGetData};
use cloned::cloned;
use context::{CoreContext, PerfCounterType};
use futures::future::{BoxFuture, FutureExt};
use futures_stats::TimedTryFutureExt;
use mononoke_types::BlobstoreBytes;
use time_ext::DurationExt;

/// A layer over an existing blobstore that respects a CoreContext's blobstore concurrency
#[derive(Clone, Debug)]
pub struct ContextConcurrencyBlobstore<T> {
    blobstore: T,
}

impl<T> ContextConcurrencyBlobstore<T> {
    pub fn as_inner(&self) -> &T {
        &self.blobstore
    }

    pub fn into_inner(self) -> T {
        self.blobstore
    }
}

#[derive(Copy, Clone)]
enum AccessReason {
    Read,
    Write,
}

async fn access(ctx: &CoreContext, reason: AccessReason) -> Result<(), Error> {
    let limiter = match reason {
        AccessReason::Read => ctx.session().blobstore_read_limiter(),
        AccessReason::Write => ctx.session().blobstore_write_limiter(),
    };

    let limiter = match limiter {
        Some(limiter) => limiter,
        None => {
            return Ok(());
        }
    };

    let (stats, ()) = limiter.access().try_timed().await?;

    let counter = match reason {
        AccessReason::Read => PerfCounterType::BlobGetsAccessWait,
        AccessReason::Write => PerfCounterType::BlobPutsAccessWait,
    };

    ctx.perf_counters()
        .add_to_counter(counter, stats.completion_time.as_micros_unchecked() as i64);

    Ok(())
}

impl<T: Blobstore + Clone> ContextConcurrencyBlobstore<T> {
    pub fn new(blobstore: T) -> Self {
        Self { blobstore }
    }
}

impl<T: Blobstore + Clone> Blobstore for ContextConcurrencyBlobstore<T> {
    fn get(
        &self,
        ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<Option<BlobstoreGetData>, Error>> {
        cloned!(self.blobstore);
        async move {
            access(&ctx, AccessReason::Read).await?;
            blobstore.get(ctx, key).await
        }
        .boxed()
    }

    fn put(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<(), Error>> {
        cloned!(self.blobstore);
        async move {
            access(&ctx, AccessReason::Write).await?;
            blobstore.put(ctx, key, value).await
        }
        .boxed()
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<'static, Result<bool, Error>> {
        cloned!(self.blobstore);
        async move {
            access(&ctx, AccessReason::Read).await?;
            blobstore.is_present(ctx, key).await
        }
        .boxed()
    }

    fn assert_present(
        &self,
        ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<(), Error>> {
        cloned!(self.blobstore);
        async move {
            access(&ctx, AccessReason::Read).await?;
            blobstore.assert_present(ctx, key).await
        }
        .boxed()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use async_limiter::AsyncLimiter;
    use context::SessionContainer;
    use fbinit::FacebookInit;
    use nonzero_ext::nonzero;
    use ratelimit_meter::{algorithms::LeakyBucket, DirectRateLimiter};
    use scuba_ext::ScubaSampleBuilder;
    use slog::{o, Drain, Level, Logger};
    use slog_glog_fmt::default_drain;
    use std::time::Duration;

    #[derive(Clone, Debug)]
    struct DummyBlob;

    impl DummyBlob {
        fn new() -> Self {
            Self
        }
    }

    impl Blobstore for DummyBlob {
        fn get(
            &self,
            _ctx: CoreContext,
            _key: String,
        ) -> BoxFuture<'static, Result<Option<BlobstoreGetData>, Error>> {
            async move { Ok(None) }.boxed()
        }

        fn put(
            &self,
            _ctx: CoreContext,
            _key: String,
            _value: BlobstoreBytes,
        ) -> BoxFuture<'static, Result<(), Error>> {
            async move { Ok(()) }.boxed()
        }

        fn is_present(
            &self,
            _ctx: CoreContext,
            _key: String,
        ) -> BoxFuture<'static, Result<bool, Error>> {
            async move { Ok(false) }.boxed()
        }
    }

    fn logger() -> Logger {
        let drain = default_drain().filter_level(Level::Debug).ignore_res();
        Logger::root(drain, o![])
    }

    #[fbinit::test]
    async fn test_qps(fb: FacebookInit) -> Result<(), Error> {
        let l1 = DirectRateLimiter::<LeakyBucket>::new(nonzero!(1u32), Duration::from_millis(10));
        let l1 = AsyncLimiter::new(l1).await;

        let l2 = DirectRateLimiter::<LeakyBucket>::new(nonzero!(1u32), Duration::from_millis(10));
        let l2 = AsyncLimiter::new(l2).await;

        let mut builder = SessionContainer::builder(fb);
        builder.blobstore_read_limiter(l1);
        builder.blobstore_write_limiter(l2);
        let session = builder.build();
        let ctx = session.new_context(logger(), ScubaSampleBuilder::with_discard());

        let blob = ContextConcurrencyBlobstore::new(DummyBlob::new());

        // get
        let (stats, _) = futures::future::try_join_all(
            (0..10usize).map(|_| blob.get(ctx.clone(), "foo".to_string())),
        )
        .try_timed()
        .await?;
        assert!(stats.completion_time.as_millis_unchecked() > 50);

        // is_present
        let (stats, _) = futures::future::try_join_all(
            (0..10usize).map(|_| blob.is_present(ctx.clone(), "foo".to_string())),
        )
        .try_timed()
        .await?;
        assert!(stats.completion_time.as_millis_unchecked() > 50);

        // put
        let bytes = BlobstoreBytes::from_bytes("test foobar");
        let (stats, _) = futures::future::try_join_all(
            (0..10usize).map(|_| blob.put(ctx.clone(), "foo".to_string(), bytes.clone())),
        )
        .try_timed()
        .await?;
        assert!(stats.completion_time.as_millis_unchecked() > 50);

        Ok(())
    }
}
