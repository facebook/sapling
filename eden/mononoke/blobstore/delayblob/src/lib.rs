/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use std::time::Duration;

use anyhow::Error;
use futures::future::FutureExt;
use futures_ext::{BoxFuture, FutureExt as OldFutureExt};
use futures_old::future::Future;
use futures_util::future::TryFutureExt;
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
    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreGetData>, Error> {
        delay(self.get_dist, self.inner.get(ctx, key)).boxify()
    }

    fn put(&self, ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        delay(self.put_dist, self.inner.put(ctx, key, value)).boxify()
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<bool, Error> {
        delay(self.get_dist, self.inner.is_present(ctx, key)).boxify()
    }

    fn assert_present(&self, ctx: CoreContext, key: String) -> BoxFuture<(), Error> {
        delay(self.get_dist, self.inner.assert_present(ctx, key)).boxify()
    }
}

fn delay<F, D>(distribution: D, target: F) -> impl Future<Item = F::Item, Error = Error>
where
    D: Distribution<f64>,
    F: Future<Error = Error>,
{
    let seconds = rand::thread_rng().sample(distribution).abs();
    async move {
        tokio::time::delay_for(Duration::new(
            seconds.trunc() as u64,
            (seconds.fract() * 1e+9) as u32,
        ))
        .await;

        let res: Result<_, Error> = Ok(());
        res
    }
    .boxed()
    .compat()
    .and_then(|()| target)
}
