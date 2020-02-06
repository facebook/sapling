/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use anyhow::Error;
use std::fmt;
use std::num::NonZeroU32;

use async_limiter::{AsyncLimiter, TokioFlavor};
use futures::future::Future;
use futures_ext::{try_boxfuture, BoxFuture, FutureExt as Futures01FutureExt};
use futures_util::future::{FutureExt, TryFutureExt};
use ratelimit_meter::{algorithms::LeakyBucket, example_algorithms::Allower, DirectRateLimiter};

use blobstore::Blobstore;
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
pub struct ThrottledBlob<T: Blobstore + Clone> {
    blobstore: T,
    read_limiter: AsyncLimiter,
    write_limiter: AsyncLimiter,
    /// The options fields are used for Debug. They are not consulted at runtime.
    options: ThrottleOptions,
}

fn limiter(qps: Option<NonZeroU32>) -> AsyncLimiter {
    match qps {
        Some(qps) => AsyncLimiter::new(
            DirectRateLimiter::<LeakyBucket>::per_second(qps),
            TokioFlavor::V01,
        ),
        None => AsyncLimiter::new(Allower::ratelimiter(), TokioFlavor::V01),
    }
}

impl<T: Blobstore + Clone> ThrottledBlob<T> {
    pub fn new(blobstore: T, options: ThrottleOptions) -> Self {
        Self {
            blobstore,
            read_limiter: limiter(options.read_qps),
            write_limiter: limiter(options.write_qps),
            options,
        }
    }
}

// NOTE: All the methods below make a clone of the Blobstore first then dispach the get after the
// limiter has allowed access, which ensures even eager work is delayed.
impl<T: Blobstore + Clone> Blobstore for ThrottledBlob<T> {
    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        let access = try_boxfuture!(self.read_limiter.access());
        let blobstore = self.blobstore.clone();
        access
            .boxed()
            .compat()
            .and_then(move |_| blobstore.get(ctx, key))
            .boxify()
    }

    fn put(&self, ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        let access = try_boxfuture!(self.write_limiter.access());
        let blobstore = self.blobstore.clone();
        access
            .boxed()
            .compat()
            .and_then(move |_| blobstore.put(ctx, key, value))
            .boxify()
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<bool, Error> {
        let access = try_boxfuture!(self.read_limiter.access());
        let blobstore = self.blobstore.clone();
        access
            .boxed()
            .compat()
            .and_then(move |_| blobstore.is_present(ctx, key))
            .boxify()
    }

    fn assert_present(&self, ctx: CoreContext, key: String) -> BoxFuture<(), Error> {
        let access = try_boxfuture!(self.read_limiter.access());
        let blobstore = self.blobstore.clone();
        access
            .boxed()
            .compat()
            .and_then(move |_| blobstore.assert_present(ctx, key))
            .boxify()
    }
}

impl<T: Blobstore + Clone> fmt::Debug for ThrottledBlob<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ThrottledBlob")
            .field("blobstore", &self.blobstore)
            .field("options", &self.options)
            .finish()
    }
}
