#![deny(warnings)]
/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZeroU64;

use anyhow::Error;
use futures_ext::BoxFuture;
use futures_old::future::{self};
use futures_stats::Timed;
use scuba::ScubaSampleBuilder;

use blobstore::{Blobstore, BlobstoreGetData};
use blobstore_stats::{record_get_stats, record_put_stats, OperationType};
use context::{CoreContext, PerfCounterType};
use mononoke_types::BlobstoreBytes;

#[derive(Debug)]
pub struct LogBlob<B> {
    inner: B,
    scuba: ScubaSampleBuilder,
    scuba_sample_rate: NonZeroU64,
}

impl<B: Blobstore> LogBlob<B> {
    pub fn new(inner: B, mut scuba: ScubaSampleBuilder, scuba_sample_rate: NonZeroU64) -> Self {
        scuba.add_common_server_data();
        Self {
            inner,
            scuba,
            scuba_sample_rate,
        }
    }
}

impl<B: Blobstore> Blobstore for LogBlob<B> {
    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreGetData>, Error> {
        let mut scuba = self.scuba.clone();
        scuba.sampled(self.scuba_sample_rate);

        ctx.perf_counters()
            .increment_counter(PerfCounterType::BlobGets);

        self.inner
            .get(ctx.clone(), key.clone())
            .timed(move |stats, result| {
                record_get_stats(
                    &mut scuba,
                    stats,
                    result,
                    key,
                    ctx.session_id().to_string(),
                    OperationType::Get,
                    None,
                );
                future::ok(())
            })
    }

    fn put(&self, ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        let mut scuba = self.scuba.clone();
        let size = value.len();

        ctx.perf_counters()
            .increment_counter(PerfCounterType::BlobPuts);

        self.inner
            .put(ctx.clone(), key.clone(), value)
            .timed(move |stats, result| {
                record_put_stats(
                    &mut scuba,
                    stats,
                    result,
                    key,
                    ctx.session_id().to_string(),
                    OperationType::Put,
                    size,
                    None,
                    None,
                );
                Ok(())
            })
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<bool, Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::BlobPresenceChecks);
        self.inner.is_present(ctx, key)
    }

    fn assert_present(&self, ctx: CoreContext, key: String) -> BoxFuture<(), Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::BlobPresenceChecks);
        self.inner.assert_present(ctx, key)
    }
}
