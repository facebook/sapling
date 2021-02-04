/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::Result;
use async_trait::async_trait;
use governor::{
    clock::DefaultClock,
    state::{direct::NotKeyed, InMemoryState},
    Jitter, Quota, RateLimiter,
};
use std::{fmt, num::NonZeroU32, time::Duration};

use blobstore::{Blobstore, BlobstoreGetData, BlobstorePutOps, OverwriteStatus, PutBehaviour};
use context::CoreContext;
use mononoke_types::BlobstoreBytes;

#[derive(Clone, Copy, Debug, Default)]
pub struct ThrottleOptions {
    pub read_qps: Option<NonZeroU32>,
    pub write_qps: Option<NonZeroU32>,
}

impl ThrottleOptions {
    pub fn has_throttle(&self) -> bool {
        self.read_qps.is_some() || self.write_qps.is_some()
    }
}

/// A Blobstore that rate limits the number of read and write operations.
pub struct ThrottledBlob<T: fmt::Debug> {
    blobstore: T,
    read_qps_limiter: Option<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    write_qps_limiter: Option<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    /// The options fields are used for Debug. They are not consulted at runtime.
    options: ThrottleOptions,
}

static JITTER_MAX: Duration = Duration::from_millis(5);

fn jitter() -> Jitter {
    Jitter::up_to(JITTER_MAX)
}

impl<T: fmt::Debug + Send + Sync> ThrottledBlob<T> {
    pub async fn new(blobstore: T, options: ThrottleOptions) -> Self {
        let read_qps_limiter = options
            .read_qps
            .map(|qps| RateLimiter::direct(Quota::per_second(qps)));
        let write_qps_limiter = options
            .write_qps
            .map(|qps| RateLimiter::direct(Quota::per_second(qps)));
        Self {
            blobstore,
            read_qps_limiter,
            write_qps_limiter,
            options,
        }
    }
}

#[async_trait]
impl<T: Blobstore> Blobstore for ThrottledBlob<T> {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        if let Some(limiter) = self.read_qps_limiter.as_ref() {
            limiter.until_ready_with_jitter(jitter()).await;
        }
        self.blobstore.get(ctx, key).await
    }

    async fn put<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        if let Some(limiter) = self.write_qps_limiter.as_ref() {
            limiter.until_ready_with_jitter(jitter()).await;
        }
        self.blobstore.put(ctx, key, value).await
    }

    async fn is_present<'a>(&'a self, ctx: &'a CoreContext, key: &'a str) -> Result<bool> {
        if let Some(limiter) = self.read_qps_limiter.as_ref() {
            limiter.until_ready_with_jitter(jitter()).await;
        }
        self.blobstore.is_present(ctx, key).await
    }
}

#[async_trait]
impl<T: BlobstorePutOps> BlobstorePutOps for ThrottledBlob<T> {
    async fn put_explicit<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: PutBehaviour,
    ) -> Result<OverwriteStatus> {
        if let Some(limiter) = self.write_qps_limiter.as_ref() {
            limiter.until_ready_with_jitter(jitter()).await;
        }
        self.blobstore
            .put_explicit(ctx, key, value, put_behaviour)
            .await
    }

    async fn put_with_status<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<OverwriteStatus> {
        if let Some(limiter) = self.write_qps_limiter.as_ref() {
            limiter.until_ready_with_jitter(jitter()).await;
        }
        self.blobstore.put_with_status(ctx, key, value).await
    }
}

impl<T: fmt::Debug> fmt::Debug for ThrottledBlob<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ThrottledBlob")
            .field("blobstore", &self.blobstore)
            .field("options", &self.options)
            .finish()
    }
}
