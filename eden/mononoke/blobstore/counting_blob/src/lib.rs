/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! A blobstore wrapper that counts operations via shared atomic counters.
//!
//! Unlike `CountedBlobstore` (ODS timeseries) or `LogBlob` (CoreContext perf
//! counters), this wrapper exposes operation counts programmatically through
//! `BlobstoreCounters`, making it suitable for tests and benchmarks that need
//! to assert on the number of blobstore operations performed.

use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreBytes;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore::OverwriteStatus;
use blobstore::PutBehaviour;
use context::CoreContext;

/// Shared atomic counters for blobstore operations.
#[derive(Debug, Default)]
pub struct BlobstoreCounters {
    pub gets: AtomicU64,
    pub puts: AtomicU64,
    pub is_presents: AtomicU64,
}

/// Snapshot of blobstore operation counts at a point in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlobstoreCountersSnapshot {
    pub gets: u64,
    pub puts: u64,
    pub is_presents: u64,
}

impl std::ops::Sub for BlobstoreCountersSnapshot {
    type Output = BlobstoreCountersSnapshot;

    fn sub(self, rhs: BlobstoreCountersSnapshot) -> BlobstoreCountersSnapshot {
        BlobstoreCountersSnapshot {
            gets: self.gets - rhs.gets,
            puts: self.puts - rhs.puts,
            is_presents: self.is_presents - rhs.is_presents,
        }
    }
}

impl BlobstoreCounters {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&self) {
        self.gets.store(0, Ordering::SeqCst);
        self.puts.store(0, Ordering::SeqCst);
        self.is_presents.store(0, Ordering::SeqCst);
    }

    pub fn snapshot(&self) -> BlobstoreCountersSnapshot {
        BlobstoreCountersSnapshot {
            gets: self.gets.load(Ordering::SeqCst),
            puts: self.puts.load(Ordering::SeqCst),
            is_presents: self.is_presents.load(Ordering::SeqCst),
        }
    }
}

/// A blobstore wrapper that counts operations via shared atomic counters.
#[derive(Debug)]
pub struct CountingBlobstore<B> {
    inner: B,
    counters: Arc<BlobstoreCounters>,
}

impl<B: std::fmt::Display> std::fmt::Display for CountingBlobstore<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "CountingBlobstore<{}>", &self.inner)
    }
}

impl<B> CountingBlobstore<B> {
    pub fn new(inner: B, counters: Arc<BlobstoreCounters>) -> Self {
        Self { inner, counters }
    }
}

#[async_trait]
impl<B: Blobstore> Blobstore for CountingBlobstore<B> {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        self.counters.gets.fetch_add(1, Ordering::SeqCst);
        self.inner.get(ctx, key).await
    }

    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        self.counters.is_presents.fetch_add(1, Ordering::SeqCst);
        self.inner.is_present(ctx, key).await
    }

    async fn unlink<'a>(&'a self, ctx: &'a CoreContext, key: &'a str) -> Result<()> {
        self.inner.unlink(ctx, key).await
    }

    async fn put_explicit<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: PutBehaviour,
    ) -> Result<OverwriteStatus> {
        self.counters.puts.fetch_add(1, Ordering::SeqCst);
        self.inner
            .put_explicit(ctx, key, value, put_behaviour)
            .await
    }

    async fn put_with_status<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<OverwriteStatus> {
        self.counters.puts.fetch_add(1, Ordering::SeqCst);
        self.inner.put_with_status(ctx, key, value).await
    }
}
