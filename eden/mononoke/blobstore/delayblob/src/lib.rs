/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore::KeyedBlobstore;
use blobstore::OverwriteStatus;
use blobstore::PutBehaviour;
use context::CoreContext;
use mononoke_types::BlobstoreBytes;
use rand::Rng;
use rand_distr::Distribution;

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

    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        delay(self.get_dist).await;
        self.inner.is_present(ctx, key).await
    }

    async fn unlink<'a>(&'a self, ctx: &'a CoreContext, key: &'a str) -> Result<()> {
        delay(self.put_dist).await;
        self.inner.unlink(ctx, key).await
    }

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
        delay(self.put_dist).await;
        self.put_impl(ctx, key, value, None).await
    }
}

impl<T: Blobstore> DelayedBlobstore<T> {
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

#[derive(Debug)]
pub struct DelayedKeyedBlobstore<B> {
    inner: B,
    get_dist: Option<Normal>,
    put_dist: Option<Normal>,
}

impl<B: std::fmt::Display> std::fmt::Display for DelayedKeyedBlobstore<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "DelayedKeyedBlobstore<{}>", &self.inner)
    }
}

impl<B> DelayedKeyedBlobstore<B> {
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
impl<B: Blobstore> KeyedBlobstore for DelayedKeyedBlobstore<B> {
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
        self.inner.put(ctx, key, value).await?;
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

    async fn unlink<'a>(&'a self, ctx: &'a CoreContext, key: &'a str) -> Result<()> {
        delay(self.put_dist).await;
        self.inner.unlink(ctx, key).await
    }

    async fn copy<'a>(
        &'a self,
        ctx: &'a CoreContext,
        old_key: &'a str,
        new_key: String,
    ) -> Result<()> {
        delay(self.put_dist).await;
        let value = self
            .inner
            .get(ctx, old_key)
            .await?
            .with_context(|| format!("key {} not present", old_key))?;
        self.inner.put(ctx, new_key, value.into_bytes()).await
    }
}
