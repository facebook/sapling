// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::sync::Arc;

use bytes::Bytes;

use cachelib::LruCachePool;
use futures::IntoFuture;
use futures_ext::{BoxFuture, FutureExt};
use locking_cache::CacheOps;
use mononoke_types::BlobstoreBytes;

use Blobstore;
use counted_blobstore::CountedBlobstore;
use dummy_lease::DummyLease;
use locking_cache::CacheBlobstore;

const CACHELIB_MAX_SIZE: usize = 1024000;

/// A caching layer over an existing blobstore, backed by cachelib
#[derive(Clone)]
pub struct CachelibOps {
    blob_pool: Arc<LruCachePool>,
    presence_pool: Arc<LruCachePool>,
}

impl CachelibOps {
    pub fn new(blob_pool: Arc<LruCachePool>, presence_pool: Arc<LruCachePool>) -> Self {
        Self {
            blob_pool,
            presence_pool,
        }
    }
}

pub fn new_cachelib_blobstore_no_lease<T>(
    blobstore: T,
    blob_pool: Arc<LruCachePool>,
    presence_pool: Arc<LruCachePool>,
) -> CountedBlobstore<CacheBlobstore<CachelibOps, DummyLease, T>>
where
    T: Blobstore + Clone,
{
    let cache_ops = CachelibOps::new(blob_pool, presence_pool);
    CountedBlobstore::new(
        "cachelib",
        CacheBlobstore::new(cache_ops, DummyLease {}, blobstore),
    )
}

impl CacheOps for CachelibOps {
    fn get(&self, key: &str) -> BoxFuture<Option<BlobstoreBytes>, ()> {
        Ok(self.blob_pool.get(key).map(BlobstoreBytes::from_bytes))
            .into_future()
            .boxify()
    }

    fn put(&self, key: &str, value: BlobstoreBytes) -> BoxFuture<(), ()> {
        self.presence_pool.set(key, Bytes::from(b"P".as_ref()));
        if value.len() < CACHELIB_MAX_SIZE {
            self.blob_pool.set(key, value.into_bytes());
        }
        Ok(()).into_future().boxify()
    }

    /// Ask the cache if it knows whether the backing store has a value for this key. Returns
    /// `true` if there is definitely a value (i.e. cache entry in Present or Known state), `false`
    /// otherwise (Empty or Leased states).
    fn check_present(&self, key: &str) -> BoxFuture<bool, ()> {
        Ok(self.presence_pool.get(key).is_some() || self.blob_pool.get(key).is_some())
            .into_future()
            .boxify()
    }
}
