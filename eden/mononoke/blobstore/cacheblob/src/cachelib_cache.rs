/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::sync::Arc;

use bytes::Bytes;

use crate::locking_cache::CacheOps;
use cachelib::LruCachePool;
use context::PerfCounterType;
use futures_ext::{BoxFuture, FutureExt};
use futures_old::IntoFuture;

use blobstore::{Blobstore, BlobstoreGetData, CountedBlobstore};

use crate::dummy::DummyLease;
use crate::in_process_lease::InProcessLease;
use crate::locking_cache::CacheBlobstore;

const MAX_CACHELIB_VALUE_SIZE: u64 = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub struct CachelibBlobstoreOptions {
    // Whether to attempt zstd compressing data so it will fit inside cachelibs threshold
    pub attempt_zstd: bool,
    // Whether to wait for cache write before returning. Usually false apart from tests.
    pub lazy_cache_put: bool,
}

impl CachelibBlobstoreOptions {
    pub fn new_lazy(attempt_zstd: Option<bool>) -> Self {
        Self {
            attempt_zstd: attempt_zstd.unwrap_or(true),
            lazy_cache_put: true,
        }
    }
    pub fn new_eager(attempt_zstd: Option<bool>) -> Self {
        Self {
            attempt_zstd: attempt_zstd.unwrap_or(true),
            lazy_cache_put: false,
        }
    }
}

impl Default for CachelibBlobstoreOptions {
    fn default() -> Self {
        Self::new_lazy(None)
    }
}

/// A caching layer over an existing blobstore, backed by cachelib
#[derive(Clone)]
pub struct CachelibOps {
    blob_pool: Arc<LruCachePool>,
    presence_pool: Arc<LruCachePool>,
    options: CachelibBlobstoreOptions,
}

impl CachelibOps {
    pub fn new(
        blob_pool: Arc<LruCachePool>,
        presence_pool: Arc<LruCachePool>,
        options: CachelibBlobstoreOptions,
    ) -> Self {
        Self {
            blob_pool,
            presence_pool,
            options,
        }
    }
}

pub fn new_cachelib_blobstore_no_lease<T>(
    blobstore: T,
    blob_pool: Arc<LruCachePool>,
    presence_pool: Arc<LruCachePool>,
    options: CachelibBlobstoreOptions,
) -> CountedBlobstore<CacheBlobstore<CachelibOps, DummyLease, T>>
where
    T: Blobstore + Clone,
{
    let cache_ops = CachelibOps::new(blob_pool, presence_pool, options);
    CountedBlobstore::new(
        "cachelib".to_string(),
        CacheBlobstore::new(cache_ops, DummyLease {}, blobstore, options.lazy_cache_put),
    )
}

pub fn new_cachelib_blobstore<T>(
    blobstore: T,
    blob_pool: Arc<LruCachePool>,
    presence_pool: Arc<LruCachePool>,
    options: CachelibBlobstoreOptions,
) -> CountedBlobstore<CacheBlobstore<CachelibOps, InProcessLease, T>>
where
    T: Blobstore + Clone,
{
    let cache_ops = CachelibOps::new(blob_pool, presence_pool, options);
    CountedBlobstore::new(
        "cachelib".to_string(),
        CacheBlobstore::new(
            cache_ops,
            InProcessLease::new(),
            blobstore,
            options.lazy_cache_put,
        ),
    )
}

impl CacheOps for CachelibOps {
    const HIT_COUNTER: Option<PerfCounterType> = Some(PerfCounterType::CachelibHits);
    const MISS_COUNTER: Option<PerfCounterType> = Some(PerfCounterType::CachelibMisses);
    const CACHE_NAME: &'static str = "cachelib";

    fn get(&self, key: &str) -> BoxFuture<Option<BlobstoreGetData>, ()> {
        self.blob_pool
            .get(key)
            .map_err(|_| ())
            .and_then(|opt| opt.map(BlobstoreGetData::decode).transpose())
            .into_future()
            .boxify()
    }

    fn put(&self, key: &str, value: BlobstoreGetData) -> BoxFuture<(), ()> {
        // A failure to set presence is considered fine, here.
        let _ = self.presence_pool.set(key, Bytes::from(b"P".as_ref()));

        let encode_limit = if self.options.attempt_zstd {
            Some(MAX_CACHELIB_VALUE_SIZE)
        } else {
            None
        };
        value
            .encode(encode_limit)
            .and_then(|bytes| self.blob_pool.set(key, bytes).map(|_| ()).map_err(|_| ()))
            .into_future()
            .boxify()
    }

    /// Ask the cache if it knows whether the backing store has a value for this key. Returns
    /// `true` if there is definitely a value (i.e. cache entry in Present or Known state), `false`
    /// otherwise (Empty or Leased states).
    fn check_present(&self, key: &str) -> BoxFuture<bool, ()> {
        let presence_pool = self
            .presence_pool
            .get(key)
            .map(|opt| opt.is_some())
            .unwrap_or(false);
        let blob_pool = self
            .blob_pool
            .get(key)
            .map(|opt| opt.is_some())
            .unwrap_or(false);

        Ok(presence_pool || blob_pool).into_future().boxify()
    }
}

impl fmt::Debug for CachelibOps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // XXX possibly add more debug info here
        write!(f, "CachelibOps")
    }
}
