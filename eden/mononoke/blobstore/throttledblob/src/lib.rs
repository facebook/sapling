/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::Error;
use std::fmt;
use std::num::NonZeroU32;

use async_limiter::AsyncLimiter;
use futures::future::{BoxFuture, FutureExt};
use ratelimit_meter::{algorithms::LeakyBucket, example_algorithms::Allower, DirectRateLimiter};

use blobstore::{Blobstore, BlobstoreGetData, BlobstorePutOps, OverwriteStatus, PutBehaviour};
use context::CoreContext;
use mononoke_types::BlobstoreBytes;

#[derive(Clone, Copy, Debug)]
pub struct ThrottleOptions {
    read_qps: Option<NonZeroU32>,
    write_qps: Option<NonZeroU32>,
}

impl ThrottleOptions {
    pub fn new(read_qps: Option<NonZeroU32>, write_qps: Option<NonZeroU32>) -> Self {
        Self {
            read_qps,
            write_qps,
        }
    }

    pub fn has_throttle(&self) -> bool {
        self.read_qps.is_some() || self.write_qps.is_some()
    }
}

/// A Blobstore that rate limits the number of read and write operations.
#[derive(Clone)]
pub struct ThrottledBlob<T: Clone + fmt::Debug> {
    blobstore: T,
    read_limiter: AsyncLimiter,
    write_limiter: AsyncLimiter,
    /// The options fields are used for Debug. They are not consulted at runtime.
    options: ThrottleOptions,
}

async fn limiter(qps: Option<NonZeroU32>) -> AsyncLimiter {
    match qps {
        Some(qps) => AsyncLimiter::new(DirectRateLimiter::<LeakyBucket>::per_second(qps)).await,
        None => AsyncLimiter::new(Allower::ratelimiter()).await,
    }
}

impl<T: Clone + fmt::Debug + Send + Sync + 'static> ThrottledBlob<T> {
    pub async fn new(blobstore: T, options: ThrottleOptions) -> Self {
        Self {
            blobstore,
            read_limiter: limiter(options.read_qps).await,
            write_limiter: limiter(options.write_qps).await,
            options,
        }
    }

    fn throttled_access<ThrottledFn, Out>(
        &self,
        limiter: &AsyncLimiter,
        throttled_fn: ThrottledFn,
    ) -> BoxFuture<'static, Result<Out, Error>>
    where
        ThrottledFn: FnOnce(T) -> BoxFuture<'static, Result<Out, Error>> + Send + 'static,
    {
        let access = limiter.access();
        // NOTE: Make a clone of the Blobstore first then dispatch after the
        // limiter has allowed access, which ensures even eager work is delayed.
        let blobstore = self.blobstore.clone();
        async move {
            access.await?;
            throttled_fn(blobstore).await
        }
        .boxed()
    }
}

// All delegate to throttled_access, which ensures even eager methods are throttled
impl<T: Blobstore + Clone> Blobstore for ThrottledBlob<T> {
    fn get(
        &self,
        ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<Option<BlobstoreGetData>, Error>> {
        self.throttled_access(&self.read_limiter, move |blobstore| blobstore.get(ctx, key))
    }

    fn put(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<(), Error>> {
        self.throttled_access(&self.write_limiter, move |blobstore| {
            blobstore.put(ctx, key, value)
        })
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<'static, Result<bool, Error>> {
        self.throttled_access(&self.read_limiter, move |blobstore| {
            blobstore.is_present(ctx, key)
        })
    }
}

// All delegate to throttled_access, which ensures even eager methods are throttled
impl<T: BlobstorePutOps + Clone + Send + Sync + 'static> BlobstorePutOps for ThrottledBlob<T> {
    fn put_explicit(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: PutBehaviour,
    ) -> BoxFuture<'static, Result<OverwriteStatus, Error>> {
        self.throttled_access(&self.write_limiter, move |blobstore| {
            blobstore.put_explicit(ctx, key, value, put_behaviour)
        })
    }

    fn put_with_status(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<OverwriteStatus, Error>> {
        self.throttled_access(&self.write_limiter, move |blobstore| {
            blobstore.put_with_status(ctx, key, value)
        })
    }
}

impl<T: Clone + fmt::Debug> fmt::Debug for ThrottledBlob<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ThrottledBlob")
            .field("blobstore", &self.blobstore)
            .field("options", &self.options)
            .finish()
    }
}
