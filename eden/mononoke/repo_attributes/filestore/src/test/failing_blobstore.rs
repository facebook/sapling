/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore::KeyedBlobstore;
use context::CoreContext;
use mononoke_types::BlobstoreBytes;
use rand::Rng;
use rand::rng;
use thiserror::Error;

#[derive(Debug, Error)]
#[error("Failing Blobstore Error")]
pub struct FailingBlobstoreError;

#[derive(Debug, Clone)]
pub struct FailingBlobstore<B> {
    inner: B,
    read_success_probability: f64,
    write_success_probability: f64,
}

impl<B> FailingBlobstore<B> {
    pub fn new(inner: B, read_success_probability: f64, write_success_probability: f64) -> Self {
        Self {
            inner,
            read_success_probability,
            write_success_probability,
        }
    }
}

impl<B: std::fmt::Display> std::fmt::Display for FailingBlobstore<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "FailingBlobstore<{}>", &self.inner)
    }
}

#[async_trait]
impl<B: KeyedBlobstore> KeyedBlobstore for FailingBlobstore<B> {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        if rng().random_bool(self.read_success_probability) {
            self.inner.get(ctx, key).await
        } else {
            Err(FailingBlobstoreError.into())
        }
    }

    async fn put<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        if rng().random_bool(self.write_success_probability) {
            self.inner.put(ctx, key, value).await
        } else {
            Err(FailingBlobstoreError.into())
        }
    }

    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        if rng().random_bool(self.read_success_probability) {
            self.inner.is_present(ctx, key).await
        } else {
            Err(FailingBlobstoreError.into())
        }
    }

    async fn copy<'a>(
        &'a self,
        ctx: &'a CoreContext,
        old_key: &'a str,
        new_key: String,
    ) -> Result<()> {
        if rng().random_bool(self.write_success_probability) {
            self.inner.copy(ctx, old_key, new_key).await
        } else {
            Err(FailingBlobstoreError.into())
        }
    }

    async fn unlink<'a>(&'a self, ctx: &'a CoreContext, key: &'a str) -> Result<()> {
        if rng().random_bool(self.read_success_probability) {
            self.inner.unlink(ctx, key).await
        } else {
            Err(FailingBlobstoreError.into())
        }
    }
}
