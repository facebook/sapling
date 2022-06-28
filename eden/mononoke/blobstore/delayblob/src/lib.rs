/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use rand::Rng;
use rand_distr::Distribution;

use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore::BlobstorePutOps;
use blobstore::OverwriteStatus;
use blobstore::PutBehaviour;
use context::CoreContext;
use mononoke_types::BlobstoreBytes;

pub type Normal = rand_distr::Normal<f64>;

#[derive(Clone, Copy, Default, Debug)]
pub struct DelayOptions {
    pub get_dist: Option<Normal>,
    pub put_dist: Option<Normal>,
}

impl DelayOptions {
    pub fn has_delay(&self) -> bool {
        self.get_dist.is_some() || self.put_dist.is_some()
    }
}

#[derive(Debug)]
pub struct DelayedBlobstore<B> {
    inner: B,
    get_dist: Option<Normal>,
    put_dist: Option<Normal>,
}

impl<B: std::fmt::Display> std::fmt::Display for DelayedBlobstore<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "DelayedBlobstore<{}>", &self.inner)
    }
}

impl<B> DelayedBlobstore<B> {
    pub fn new(inner: B, get_dist: Normal, put_dist: Normal) -> Self {
        Self {
            inner,
            get_dist: Some(get_dist),
            put_dist: Some(put_dist),
        }
    }

    pub fn from_options(inner: B, options: DelayOptions) -> Self {
        Self {
            inner,
            get_dist: options.get_dist,
            put_dist: options.put_dist,
        }
    }
}

#[async_trait]
impl<B: BlobstorePutOps> Blobstore for DelayedBlobstore<B> {
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
        self.put_impl(ctx, key, value, None).await?;
        Ok(())
    }

    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        delay(self.get_dist).await;
        self.inner.is_present(ctx, key).await
    }
}

impl<T: BlobstorePutOps> DelayedBlobstore<T> {
    async fn put_impl<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: Option<PutBehaviour>,
    ) -> Result<OverwriteStatus> {
        delay(self.put_dist).await;

        if let Some(put_behaviour) = put_behaviour {
            self.inner
                .put_explicit(ctx, key.clone(), value, put_behaviour)
                .await
        } else {
            self.inner.put_with_status(ctx, key.clone(), value).await
        }
    }
}

#[async_trait]
impl<T: BlobstorePutOps> BlobstorePutOps for DelayedBlobstore<T> {
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

async fn delay<D>(distribution: Option<D>)
where
    D: Distribution<f64>,
{
    if let Some(distribution) = distribution {
        let seconds = rand::thread_rng().sample(distribution).abs();
        tokio::time::sleep(Duration::new(
            seconds.trunc() as u64,
            (seconds.fract() * 1e+9) as u32,
        ))
        .await;
    }
}
