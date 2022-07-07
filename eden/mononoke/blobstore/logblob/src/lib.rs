/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZeroU64;

use anyhow::Result;
use async_trait::async_trait;
use futures_stats::TimedFutureExt;
use scuba_ext::MononokeScubaSampleBuilder;

use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore::BlobstorePutOps;
use blobstore::OverwriteStatus;
use blobstore::PutBehaviour;
use blobstore_stats::record_get_stats;
use blobstore_stats::record_is_present_stats;
use blobstore_stats::record_put_stats;
use blobstore_stats::OperationType;
use context::CoreContext;
use context::PerfCounterType;
use mononoke_types::BlobstoreBytes;

#[derive(Debug)]
pub struct LogBlob<B> {
    inner: B,
    scuba: MononokeScubaSampleBuilder,
    scuba_sample_rate: NonZeroU64,
}

impl<B: std::fmt::Debug> LogBlob<B> {
    pub fn new(
        inner: B,
        mut scuba: MononokeScubaSampleBuilder,
        scuba_sample_rate: NonZeroU64,
    ) -> Self {
        scuba.add_common_server_data();
        Self {
            inner,
            scuba,
            scuba_sample_rate,
        }
    }
}

impl<T: std::fmt::Display> std::fmt::Display for LogBlob<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LogBlob<{}>", &self.inner)
    }
}

#[async_trait]
impl<B: Blobstore + BlobstorePutOps> Blobstore for LogBlob<B> {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        let mut ctx = ctx.clone();
        let mut scuba = self.scuba.clone();
        scuba.sampled(self.scuba_sample_rate);

        ctx.perf_counters()
            .increment_counter(PerfCounterType::BlobGets);

        let pc = ctx.fork_perf_counters();

        let get = self.inner.get(&ctx, key);
        let (stats, result) = get.timed().await;
        record_get_stats(
            &mut scuba,
            &pc,
            stats,
            result.as_ref(),
            key,
            ctx.metadata().session_id().as_str(),
            OperationType::Get,
            None,
            &self.inner,
        );

        match result {
            Ok(Some(ref data)) => {
                ctx.perf_counters().add_to_counter(
                    PerfCounterType::BlobGetsTotalSize,
                    data.len().try_into().unwrap_or(0),
                );
            }
            _ => {}
        }

        result
    }

    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        let mut ctx = ctx.clone();
        let mut scuba = self.scuba.clone();
        scuba.sampled(self.scuba_sample_rate);

        ctx.perf_counters()
            .increment_counter(PerfCounterType::BlobPresenceChecks);

        let pc = ctx.fork_perf_counters();

        let is_present = self.inner.is_present(&ctx, key);
        let (stats, result) = is_present.timed().await;
        record_is_present_stats(
            &mut scuba,
            &pc,
            stats,
            result.as_ref(),
            key,
            ctx.metadata().session_id().as_str(),
            None,
            &self.inner,
        );

        result
    }

    async fn put<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        BlobstorePutOps::put_with_status(self, ctx, key, value).await?;
        Ok(())
    }
}

impl<B: BlobstorePutOps> LogBlob<B> {
    async fn put_impl<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: Option<PutBehaviour>,
    ) -> Result<OverwriteStatus> {
        let mut ctx = ctx.clone();
        let mut scuba = self.scuba.clone();
        let size = value.len();

        ctx.perf_counters()
            .increment_counter(PerfCounterType::BlobPuts);

        let pc = ctx.fork_perf_counters();

        let put = if let Some(put_behaviour) = put_behaviour {
            self.inner
                .put_explicit(&ctx, key.clone(), value, put_behaviour)
        } else {
            self.inner.put_with_status(&ctx, key.clone(), value)
        };
        let (stats, result) = put.timed().await;
        record_put_stats(
            &mut scuba,
            &pc,
            stats,
            result.as_ref(),
            &key,
            ctx.metadata().session_id().as_str(),
            size,
            None,
            &self.inner,
            None,
        );

        if result.is_ok() {
            ctx.perf_counters().add_to_counter(
                PerfCounterType::BlobPutsTotalSize,
                size.try_into().unwrap_or(0),
            );
        }

        result
    }
}

#[async_trait]
impl<B: BlobstorePutOps> BlobstorePutOps for LogBlob<B> {
    async fn put_explicit<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: PutBehaviour,
    ) -> Result<OverwriteStatus> {
        self.put_impl(ctx, key, value, Some(put_behaviour)).await
    }

    async fn put_with_status<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<OverwriteStatus> {
        self.put_impl(ctx, key, value, None).await
    }
}
