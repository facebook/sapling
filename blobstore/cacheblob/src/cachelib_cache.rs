/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::fmt;
use std::sync::Arc;

use bytes::Bytes;

use crate::locking_cache::CacheOps;
use cachelib::LruCachePool;
use futures::IntoFuture;
use futures_ext::{BoxFuture, FutureExt};
use mononoke_types::BlobstoreBytes;

use blobstore::{Blobstore, CountedBlobstore};

use crate::dummy::DummyLease;
use crate::in_process_lease::InProcessLease;
use crate::locking_cache::CacheBlobstore;

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
        "cachelib".to_string(),
        CacheBlobstore::new(cache_ops, DummyLease {}, blobstore),
    )
}

pub fn new_cachelib_blobstore<T>(
    blobstore: T,
    blob_pool: Arc<LruCachePool>,
    presence_pool: Arc<LruCachePool>,
) -> CountedBlobstore<CacheBlobstore<CachelibOps, InProcessLease, T>>
where
    T: Blobstore + Clone,
{
    let cache_ops = CachelibOps::new(blob_pool, presence_pool);
    CountedBlobstore::new(
        "cachelib".to_string(),
        CacheBlobstore::new(cache_ops, InProcessLease::new(), blobstore),
    )
}

impl CacheOps for CachelibOps {
    fn get(&self, key: &str) -> BoxFuture<Option<BlobstoreBytes>, ()> {
        self.blob_pool
            .get(key)
            .map_err(|_| ())
            .map(|opt| opt.map(BlobstoreBytes::from_bytes))
            .into_future()
            .boxify()
    }

    fn put(&self, key: &str, value: BlobstoreBytes) -> BoxFuture<(), ()> {
        // A failure to set presence is considered fine, here.
        let _ = self.presence_pool.set(key, Bytes::from(b"P".as_ref()));
        self.blob_pool
            .set(key, value.into_bytes())
            .map(|_| ())
            .map_err(|_| ())
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
