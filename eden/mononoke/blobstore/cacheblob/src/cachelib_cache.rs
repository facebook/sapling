/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreBytes;
use blobstore::BlobstoreGetData;
use blobstore::CountedBlobstore;
use bytes::Bytes;
use cachelib::LruCachePool;
use context::PerfCounterType;
use std::fmt;
use std::sync::Arc;

use crate::dummy::DummyLease;
use crate::in_process_lease::InProcessLease;
use crate::locking_cache::CacheBlobstore;
use crate::locking_cache::CacheOps;

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

impl std::fmt::Display for CachelibOps {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "CachelibOps")
    }
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
    T: Blobstore,
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

#[async_trait]
impl CacheOps for CachelibOps {
    const HIT_COUNTER: Option<PerfCounterType> = Some(PerfCounterType::CachelibHits);
    const MISS_COUNTER: Option<PerfCounterType> = Some(PerfCounterType::CachelibMisses);
    const CACHE_NAME: &'static str = "cachelib";

    async fn get(&self, key: &str) -> Option<BlobstoreGetData> {
        let blob = self.blob_pool.get(key);
        let blob = blob.ok()??;
        let blob = BlobstoreBytes::decode(blob).ok()?;
        Some(blob.into())
    }

    async fn put(&self, key: &str, value: BlobstoreGetData) {
        // A failure to set presence is considered fine, here.
        let _ = self.presence_pool.set(key, Bytes::from(b"P".as_ref()));

        let encode_limit = if self.options.attempt_zstd {
            Some(MAX_CACHELIB_VALUE_SIZE)
        } else {
            None
        };
        if let Ok(bytes) = value.into_bytes().encode(encode_limit) {
            let _ = self.blob_pool.set(key, bytes);
        }
    }

    /// Ask the cache if it knows whether the backing store has a value for this key. Returns
    /// `true` if there is definitely a value (i.e. cache entry in Present or Known state), `false`
    /// otherwise (Empty or Leased states).
    async fn check_present(&self, key: &str) -> bool {
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

        presence_pool || blob_pool
    }
}

impl fmt::Debug for CachelibOps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // XXX possibly add more debug info here
        write!(f, "CachelibOps")
    }
}
