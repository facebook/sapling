/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use std::time::Duration;

use anyhow::Result;
use futures::future::{BoxFuture, FutureExt};
use rand::Rng;
use rand_distr::Distribution;

use blobstore::{Blobstore, BlobstoreGetData};
use context::CoreContext;
use mononoke_types::BlobstoreBytes;

pub type Normal = rand_distr::Normal<f64>;

#[derive(Debug)]
pub struct DelayedBlobstore<B> {
    inner: B,
    get_dist: Normal,
    put_dist: Normal,
}

impl<B> DelayedBlobstore<B> {
    pub fn new(inner: B, get_dist: Normal, put_dist: Normal) -> Self {
        Self {
            inner,
            get_dist,
            put_dist,
        }
    }
}

impl<B: Blobstore> Blobstore for DelayedBlobstore<B> {
    fn get(
        &self,
        ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'_, Result<Option<BlobstoreGetData>>> {
        let delay = delay(self.get_dist);
        let get = self.inner.get(ctx, key);
        async move {
            delay.await;
            get.await
        }
        .boxed()
    }

    fn put(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'_, Result<()>> {
        let delay = delay(self.put_dist);
        let put = self.inner.put(ctx, key, value);
        async move {
            delay.await;
            put.await
        }
        .boxed()
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<'_, Result<bool>> {
        let delay = delay(self.get_dist);
        let is_present = self.inner.is_present(ctx, key);
        async move {
            delay.await;
            is_present.await
        }
        .boxed()
    }
}

async fn delay<D>(distribution: D)
where
    D: Distribution<f64>,
{
    let seconds = rand::thread_rng().sample(distribution).abs();
    tokio::time::delay_for(Duration::new(
        seconds.trunc() as u64,
        (seconds.fract() * 1e+9) as u32,
    ))
    .await;
}
