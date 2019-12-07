/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::ops::Deref;
use std::sync::Arc;

use anyhow::Error;
use futures::Future;
use futures_ext::{BoxFuture, FutureExt};
use stats::{define_stats_struct, Timeseries};

use context::CoreContext;

use crate::{Blobstore, BlobstoreBytes};

define_stats_struct! {
    CountedBlobstoreStats("mononoke.blobstore.{}", prefix: String),
    get: timeseries(RATE, SUM),
    get_miss: timeseries(RATE, SUM),
    get_hit: timeseries(RATE, SUM),
    get_err: timeseries(RATE, SUM),
    put: timeseries(RATE, SUM),
    put_ok: timeseries(RATE, SUM),
    put_err: timeseries(RATE, SUM),
    is_present: timeseries(RATE, SUM),
    is_present_miss: timeseries(RATE, SUM),
    is_present_hit: timeseries(RATE, SUM),
    is_present_err: timeseries(RATE, SUM),
    assert_present: timeseries(RATE, SUM),
    assert_present_ok: timeseries(RATE, SUM),
    assert_present_err: timeseries(RATE, SUM),
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
    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        let stats = self.stats.clone();
        stats.get.add_value(1);
        self.blobstore
            .get(ctx, key)
            .then(move |res| {
                match res {
                    Ok(Some(_)) => stats.get_hit.add_value(1),
                    Ok(None) => stats.get_miss.add_value(1),
                    Err(_) => stats.get_err.add_value(1),
                }
                res
            })
            .boxify()
    }

    fn put(&self, ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        let stats = self.stats.clone();
        stats.put.add_value(1);
        self.blobstore
            .put(ctx, key, value)
            .then(move |res| {
                match res {
                    Ok(()) => stats.put_ok.add_value(1),
                    Err(_) => stats.put_err.add_value(1),
                }
                res
            })
            .boxify()
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<bool, Error> {
        let stats = self.stats.clone();
        stats.is_present.add_value(1);
        self.blobstore
            .is_present(ctx, key)
            .then(move |res| {
                match res {
                    Ok(true) => stats.is_present_hit.add_value(1),
                    Ok(false) => stats.is_present_miss.add_value(1),
                    Err(_) => stats.is_present_err.add_value(1),
                }
                res
            })
            .boxify()
    }

    fn assert_present(&self, ctx: CoreContext, key: String) -> BoxFuture<(), Error> {
        let stats = self.stats.clone();
        stats.assert_present.add_value(1);
        self.blobstore
            .assert_present(ctx, key)
            .then(move |res| {
                match res {
                    Ok(()) => stats.assert_present_ok.add_value(1),
                    Err(_) => stats.assert_present_err.add_value(1),
                }
                res
            })
            .boxify()
    }
}

impl<T: Blobstore> Deref for CountedBlobstore<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.as_inner()
    }
}
