/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::Deref;
use std::sync::Arc;

use anyhow::Error;
use futures::future::{BoxFuture, FutureExt};
use stats::prelude::*;

use context::CoreContext;

use crate::{Blobstore, BlobstoreBytes, BlobstoreGetData, BlobstoreWithLink};

define_stats_struct! {
    CountedBlobstoreStats("mononoke.blobstore.{}", prefix: String),
    get: timeseries(Rate, Sum),
    get_ok: timeseries(Rate, Sum),
    get_err: timeseries(Rate, Sum),
    put: timeseries(Rate, Sum),
    put_ok: timeseries(Rate, Sum),
    put_err: timeseries(Rate, Sum),
    is_present: timeseries(Rate, Sum),
    is_present_ok: timeseries(Rate, Sum),
    is_present_err: timeseries(Rate, Sum),
    assert_present: timeseries(Rate, Sum),
    assert_present_ok: timeseries(Rate, Sum),
    assert_present_err: timeseries(Rate, Sum),
    link: timeseries(Rate, Sum),
    link_ok: timeseries(Rate, Sum),
    link_err: timeseries(Rate, Sum),
}

#[derive(Clone, Debug)]
pub struct CountedBlobstore<T: Blobstore> {
    blobstore: T,
    stats: Arc<CountedBlobstoreStats>,
}

impl<T: Blobstore> CountedBlobstore<T> {
    pub fn new(name: String, blobstore: T) -> Self {
        Self {
            blobstore,
            stats: Arc::new(CountedBlobstoreStats::new(name)),
        }
    }

    pub fn into_inner(self) -> T {
        self.blobstore
    }

    pub fn as_inner(&self) -> &T {
        &self.blobstore
    }
}

impl<T: Blobstore> Blobstore for CountedBlobstore<T> {
    fn get(
        &self,
        ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<Option<BlobstoreGetData>, Error>> {
        let stats = self.stats.clone();
        stats.get.add_value(1);
        let get = self.blobstore.get(ctx, key);
        async move {
            let res = get.await;
            match res {
                Ok(_) => stats.get_ok.add_value(1),
                Err(_) => stats.get_err.add_value(1),
            }
            res
        }
        .boxed()
    }

    fn put(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<(), Error>> {
        let stats = self.stats.clone();
        stats.put.add_value(1);
        let put = self.blobstore.put(ctx, key, value);
        async move {
            let res = put.await;
            match res {
                Ok(()) => stats.put_ok.add_value(1),
                Err(_) => stats.put_err.add_value(1),
            }
            res
        }
        .boxed()
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<'static, Result<bool, Error>> {
        let stats = self.stats.clone();
        stats.is_present.add_value(1);
        let is_present = self.blobstore.is_present(ctx, key);
        async move {
            let res = is_present.await;
            match res {
                Ok(_) => stats.is_present_ok.add_value(1),
                Err(_) => stats.is_present_err.add_value(1),
            }
            res
        }
        .boxed()
    }

    fn assert_present(
        &self,
        ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<(), Error>> {
        let stats = self.stats.clone();
        stats.assert_present.add_value(1);
        let assert_present = self.blobstore.assert_present(ctx, key);
        async move {
            let res = assert_present.await;
            match res {
                Ok(()) => stats.assert_present_ok.add_value(1),
                Err(_) => stats.assert_present_err.add_value(1),
            }
            res
        }
        .boxed()
    }
}

impl<T: BlobstoreWithLink> BlobstoreWithLink for CountedBlobstore<T> {
    fn link(
        &self,
        ctx: CoreContext,
        existing_key: String,
        link_key: String,
    ) -> BoxFuture<'static, Result<(), Error>> {
        let stats = self.stats.clone();
        stats.link.add_value(1);
        let res = self.blobstore.link(ctx, existing_key, link_key);
        async move {
            let res = res.await;
            match res {
                Ok(()) => stats.link_ok.add_value(1),
                Err(_) => stats.link_err.add_value(1),
            }
            res
        }
        .boxed()
    }
}

impl<T: Blobstore> Deref for CountedBlobstore<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.as_inner()
    }
}
