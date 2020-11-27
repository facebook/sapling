/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
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

#[async_trait]
impl<B: Blobstore> Blobstore for DelayedBlobstore<B> {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        delay(self.get_dist).await;
        self.inner.get(ctx, key).await
    }

    async fn put<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        delay(self.put_dist).await;
        self.inner.put(ctx, key, value).await
    }

    async fn is_present<'a>(&'a self, ctx: &'a CoreContext, key: &'a str) -> Result<bool> {
        delay(self.get_dist).await;
        self.inner.is_present(ctx, key).await
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
