#![deny(warnings)]
/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZeroU64;

use anyhow::Error;
use futures::future::{BoxFuture, FutureExt, TryFutureExt};
use futures_stats::TimedFutureExt;
use scuba::ScubaSampleBuilder;

use blobstore::{Blobstore, BlobstoreGetData, BlobstorePutOps, OverwriteStatus, PutBehaviour};
use blobstore_stats::{record_get_stats, record_put_stats, OperationType};
use context::{CoreContext, PerfCounterType};
use mononoke_types::BlobstoreBytes;

#[derive(Debug)]
pub struct LogBlob<B> {
    inner: B,
    scuba: ScubaSampleBuilder,
    scuba_sample_rate: NonZeroU64,
}

impl<B> LogBlob<B> {
    pub fn new(inner: B, mut scuba: ScubaSampleBuilder, scuba_sample_rate: NonZeroU64) -> Self {
        scuba.add_common_server_data();
        Self {
            inner,
            scuba,
            scuba_sample_rate,
        }
    }
}

impl<B: Blobstore + BlobstorePutOps> Blobstore for LogBlob<B> {
    fn get(
        &self,
        mut ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<Option<BlobstoreGetData>, Error>> {
        let mut scuba = self.scuba.clone();
        scuba.sampled(self.scuba_sample_rate);

        ctx.perf_counters()
            .increment_counter(PerfCounterType::BlobGets);

        let pc = ctx.fork_perf_counters();

        let get = self.inner.get(ctx.clone(), key.clone());
        let session_id = ctx.metadata().session_id().to_string();
        async move {
            let (stats, result) = get.timed().await;
            record_get_stats(
                &mut scuba,
                &pc,
                stats,
                result.as_ref(),
                key,
                session_id,
                OperationType::Get,
                None,
            );
            result
        }
        .boxed()
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<'static, Result<bool, Error>> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::BlobPresenceChecks);
        self.inner.is_present(ctx, key)
    }

    fn put(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<(), Error>> {
        BlobstorePutOps::put_with_status(self, ctx, key, value)
            .map_ok(|_| ())
            .boxed()
    }
}

impl<B: BlobstorePutOps> LogBlob<B> {
    fn put_impl(
        &self,
        mut ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: Option<PutBehaviour>,
    ) -> BoxFuture<'static, Result<OverwriteStatus, Error>> {
        let mut scuba = self.scuba.clone();
        let size = value.len();

        ctx.perf_counters()
            .increment_counter(PerfCounterType::BlobPuts);

        let pc = ctx.fork_perf_counters();

        let put = if let Some(put_behaviour) = put_behaviour {
            self.inner
                .put_explicit(ctx.clone(), key.clone(), value, put_behaviour)
        } else {
            self.inner.put_with_status(ctx.clone(), key.clone(), value)
        };
        async move {
            let (stats, result) = put.timed().await;
            record_put_stats(
                &mut scuba,
                &pc,
                stats,
                result.as_ref(),
                key,
                ctx.metadata().session_id().to_string(),
                OperationType::Put,
                size,
                None,
                None,
            );
            result
        }
        .boxed()
    }
}

impl<B: BlobstorePutOps> BlobstorePutOps for LogBlob<B> {
    fn put_explicit(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: PutBehaviour,
    ) -> BoxFuture<'static, Result<OverwriteStatus, Error>> {
        self.put_impl(ctx, key, value, Some(put_behaviour))
    }

    fn put_with_status(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<OverwriteStatus, Error>> {
        self.put_impl(ctx, key, value, None)
    }
}
