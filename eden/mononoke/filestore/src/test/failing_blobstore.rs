/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobstore::{Blobstore, BlobstoreGetData};
use context::CoreContext;
use futures::future::{self, BoxFuture, FutureExt};
use mononoke_types::BlobstoreBytes;
use rand::{thread_rng, Rng};
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

impl<B> Blobstore for FailingBlobstore<B>
where
    B: Blobstore,
{
    fn get(
        &self,
        ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<Option<BlobstoreGetData>, Error>> {
        let mut rng = thread_rng();
        if rng.gen_bool(self.read_success_probability) {
            self.inner.get(ctx, key)
        } else {
            future::err(FailingBlobstoreError.into()).boxed()
        }
    }

    fn put(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<(), Error>> {
        let mut rng = thread_rng();
        if rng.gen_bool(self.write_success_probability) {
            self.inner.put(ctx, key, value)
        } else {
            future::err(FailingBlobstoreError.into()).boxed()
        }
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<'static, Result<bool, Error>> {
        let mut rng = thread_rng();
        if rng.gen_bool(self.read_success_probability) {
            self.inner.is_present(ctx, key)
        } else {
            future::err(FailingBlobstoreError.into()).boxed()
        }
    }

    fn assert_present(
        &self,
        ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<(), Error>> {
        let mut rng = thread_rng();
        if rng.gen_bool(self.read_success_probability) {
            self.inner.assert_present(ctx, key)
        } else {
            future::err(FailingBlobstoreError.into()).boxed()
        }
    }
}
