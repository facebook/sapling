// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure::Error;
use futures::Future;
use futures_ext::{BoxFuture, FutureExt};
use stats::DynamicTimeseries;

use mononoke_types::BlobstoreBytes;

use {Blobstore, CacheBlobstoreExt};

define_stats! {
    prefix = "mononoke.blobstore";
    get: dynamic_timeseries("{}.get", (name: &'static str); RATE, SUM),
    get_miss: dynamic_timeseries("{}.get.miss", (name: &'static str); RATE, SUM),
    get_hit: dynamic_timeseries("{}.get.hit", (name: &'static str); RATE, SUM),
    get_err: dynamic_timeseries("{}.get.err", (name: &'static str); RATE, SUM),
    put: dynamic_timeseries("{}.put", (name: &'static str); RATE, SUM),
    put_ok: dynamic_timeseries("{}.put.ok", (name: &'static str); RATE, SUM),
    put_err: dynamic_timeseries("{}.put.err", (name: &'static str); RATE, SUM),
    is_present: dynamic_timeseries("{}.is_present", (name: &'static str); RATE, SUM),
    is_present_miss: dynamic_timeseries("{}.is_present.miss", (name: &'static str); RATE, SUM),
    is_present_hit: dynamic_timeseries("{}.is_present.hit", (name: &'static str); RATE, SUM),
    is_present_err: dynamic_timeseries("{}.is_present.err", (name: &'static str); RATE, SUM),
    assert_present: dynamic_timeseries("{}.assert_present", (name: &'static str); RATE, SUM),
    assert_present_ok: dynamic_timeseries(
        "{}.assert_present.ok", (name: &'static str); RATE, SUM),
    assert_present_err: dynamic_timeseries(
        "{}.assert_present.err", (name: &'static str); RATE, SUM),
}

#[derive(Clone)]
pub struct CountedBlobstore<T: Blobstore> {
    name: &'static str,
    blobstore: T,
}

impl<T: Blobstore> CountedBlobstore<T> {
    pub fn new(name: &'static str, blobstore: T) -> Self {
        Self { name, blobstore }
    }

    pub fn into_inner(self) -> T {
        self.blobstore
    }

    pub fn as_inner(&self) -> &T {
        &self.blobstore
    }
}

impl<T: Blobstore> Blobstore for CountedBlobstore<T> {
    fn get(&self, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        let name = self.name;
        STATS::get.add_value(1, (name,));
        self.blobstore
            .get(key)
            .then(move |res| {
                match res {
                    Ok(Some(_)) => STATS::get_hit.add_value(1, (name,)),
                    Ok(None) => STATS::get_miss.add_value(1, (name,)),
                    Err(_) => STATS::get_err.add_value(1, (name,)),
                }
                res
            })
            .boxify()
    }

    fn put(&self, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        let name = self.name;
        STATS::put.add_value(1, (name,));
        self.blobstore
            .put(key, value)
            .then(move |res| {
                match res {
                    Ok(()) => STATS::put_ok.add_value(1, (name,)),
                    Err(_) => STATS::put_err.add_value(1, (name,)),
                }
                res
            })
            .boxify()
    }

    fn is_present(&self, key: String) -> BoxFuture<bool, Error> {
        let name = self.name;
        STATS::is_present.add_value(1, (name,));
        self.blobstore
            .is_present(key)
            .then(move |res| {
                match res {
                    Ok(true) => STATS::is_present_hit.add_value(1, (name,)),
                    Ok(false) => STATS::is_present_miss.add_value(1, (name,)),
                    Err(_) => STATS::is_present_err.add_value(1, (name,)),
                }
                res
            })
            .boxify()
    }

    fn assert_present(&self, key: String) -> BoxFuture<(), Error> {
        let name = self.name;
        STATS::assert_present.add_value(1, (name,));
        self.blobstore
            .assert_present(key)
            .then(move |res| {
                match res {
                    Ok(()) => STATS::assert_present_ok.add_value(1, (name,)),
                    Err(_) => STATS::assert_present_err.add_value(1, (name,)),
                }
                res
            })
            .boxify()
    }
}

impl<T: CacheBlobstoreExt> CacheBlobstoreExt for CountedBlobstore<T> {
    #[inline]
    fn get_no_cache_fill(&self, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        self.as_inner().get_no_cache_fill(key)
    }

    #[inline]
    fn get_cache_only(&self, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        self.as_inner().get_cache_only(key)
    }
}
