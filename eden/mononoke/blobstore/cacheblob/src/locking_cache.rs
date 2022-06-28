/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use auto_impl::auto_impl;
use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore::CountedBlobstore;
use cloned::cloned;
use context::CoreContext;
use context::PerfCounterType;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use mononoke_types::BlobstoreBytes;
use prefixblob::PrefixBlobstore;
use redactedblobstore::config::GET_OPERATION;
use redactedblobstore::RedactedBlobstore;
use stats::prelude::*;
use std::fmt;

define_stats! {
    prefix = "mononoke.blobstore.cacheblob";
    get_miss: dynamic_timeseries("{}.get_miss", (cache_name: &'static str); Rate, Sum),
    get_hit: dynamic_timeseries("{}.get_hit", (cache_name: &'static str); Rate, Sum),
    presence_hit: dynamic_timeseries("{}.presence_hit", (cache_name: &'static str); Rate, Sum),
    presence_miss: dynamic_timeseries("{}.presence_miss", (cache_name: &'static str); Rate, Sum),
}

/// Extra operations that can be performed on a cache. Other wrappers can implement this trait for
/// e.g. all `WrapperBlobstore<CacheBlobstore<T>>`.
///
/// This is primarily used by the admin command to manually check memcache.
#[async_trait]
pub trait CacheBlobstoreExt: Blobstore {
    async fn get_no_cache_fill<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>>;
    async fn get_cache_only<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>>;
}

/// The operations a cache must provide in order to be usable as the caching layer for a
/// caching blobstore that caches blob contents and blob presence.
/// For caches that do no I/O (e.g. in-memory caches), use Result::into_future() to create the
/// return types - it is up to CacheBlobstore to use future::lazy where this would be unacceptable
/// Errors returned by the cache are always ignored.
///
/// The cache is expected to act as-if each entry is in one of four states:
/// 1. Empty, implying that the cache has no knowledge of the backing store state for this key.
/// 2. Leased, implying that the cache is aware that there is an attempt being made to update the
///    backing store for this key.
/// 3. Present, implying that the cache is aware that a backing store entry exists for this key
///    but does not have a copy of the blob.
/// 4. Known, implying that the cache has a copy of the blob for this key.
///
/// When the cache engages in eviction, it demotes entries according to the following plan:
/// Present and Leased can only demote to Empty.
/// Known can demote to Present or Empty.
/// No state is permitted to demote to Leased.
/// Caches that do not support LeaseOps do not have the Leased state.
#[async_trait]
#[auto_impl(Arc)]
pub trait CacheOps: fmt::Display + fmt::Debug + Send + Sync {
    const HIT_COUNTER: Option<PerfCounterType> = None;
    const MISS_COUNTER: Option<PerfCounterType> = None;
    const CACHE_NAME: &'static str = "unknown";

    /// Fetch the blob from the cache, if possible. Return `None` if the cache does not have a
    /// copy of the blob (i.e. the cache entry is not in Known state).
    async fn get(&self, key: &str) -> Option<BlobstoreGetData>;

    /// Tell the cache that the backing store value for this `key` is `value`. This should put the
    /// cache entry for this `key` into Known state or a demotion of Known state (Present, Empty).
    async fn put(&self, key: &str, value: BlobstoreGetData);

    /// Ask the cache if it knows whether the backing store has a value for this key. Returns
    /// `true` if there is definitely a value (i.e. cache entry in Present or Known state), `false`
    /// otherwise (Empty or Leased states).
    async fn check_present(&self, key: &str) -> bool;
}

/// The operations a cache must provide to take part in the update lease protocol. This reduces the
/// thundering herd on writes by using the Leased state to ensure that only one user of this cache
/// can write to the backing store at any time. Note that this is not a guarantee that there will
/// be only one writer to the backing store for any given key - notably, the cache can demote
/// Leased to Empty, thus letting another writer that shares the same cache through to the backing
/// store.
#[async_trait]
#[auto_impl(Arc)]
pub trait LeaseOps: fmt::Display + fmt::Debug + Send + Sync {
    /// Ask the cache to attempt to lock out other users of this cache for a particular key.
    /// This is an atomic test-and-set of the cache entry; it tests that the entry is Empty, and if
    /// the entry is Empty, it changes it to the Leased state.
    /// The result is `true` if the test-and-set changed the entry to Leased state, `false`
    /// otherwise
    async fn try_add_put_lease(&self, key: &str) -> Result<bool>;

    /// Will keep the lease alive until `done` future resolves.
    /// Note that it should only be called after successful try_add_put_lease()
    fn renew_lease_until(&self, ctx: CoreContext, key: &str, done: BoxFuture<'static, ()>);

    /// Wait for a suitable (cache-defined) period between `try_add_put_lease` attempts.
    /// For caches without a notification method, this should just be a suitable delay.
    /// For caches that can notify on key change, this should wait for that notification.
    /// It is acceptable to return from this future without checking the state of the cache entry.
    async fn wait_for_other_leases(&self, key: &str);

    /// Releases any leases held on `key`. The entry must transition from Leased to Empty.
    async fn release_lease(&self, key: &str);
}

/// A caching layer over a blobstore, using a cache defined by its CacheOps. The idea is that
/// generic code that any caching layer needs is defined here, while code that's cache-specific
/// goes into CacheOps
#[derive(Clone)]
pub struct CacheBlobstore<C, L, T>
where
    C: CacheOps + Clone,
    L: LeaseOps + Clone,
    T: Blobstore,
{
    blobstore: T,
    cache: C,
    lease: L,
    lazy_cache_put: bool,
}

impl<C, L, T> fmt::Display for CacheBlobstore<C, L, T>
where
    C: CacheOps + Clone,
    L: LeaseOps + Clone,
    T: Blobstore,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "CacheBlobstore<{}, {}, {}>",
            &self.cache, &self.lease, &self.blobstore
        )
    }
}

impl<C, L, T> CacheBlobstore<C, L, T>
where
    C: CacheOps + Clone,
    L: LeaseOps + Clone,
    T: Blobstore,
{
    pub fn new(cache: C, lease: L, blobstore: T, lazy_cache_put: bool) -> Self {
        Self {
            blobstore,
            cache,
            lease,
            lazy_cache_put,
        }
    }

    fn take_put_lease<'a>(&'a self, key: &'a str) -> BoxFuture<'a, bool> {
        async move {
            if self.lease.try_add_put_lease(key).await.map_err(|_| ()) == Ok(true) {
                return true;
            }
            if self.cache.check_present(key).await {
                return false;
            }
            self.lease.wait_for_other_leases(key).await;
            self.take_put_lease(key).await
        }
        .boxed()
    }
}

#[async_trait]
impl<C, L, T> Blobstore for CacheBlobstore<C, L, T>
where
    C: CacheOps + Clone + 'static,
    L: LeaseOps + Clone + 'static,
    T: Blobstore,
{
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        let blob = self.cache.get(key).await;
        if blob.is_some() {
            if let Some(counter) = C::HIT_COUNTER {
                ctx.perf_counters().increment_counter(counter);
            }
            STATS::get_hit.add_value(1, (C::CACHE_NAME,));
            Ok(blob)
        } else {
            if let Some(counter) = C::MISS_COUNTER {
                ctx.perf_counters().increment_counter(counter);
            }
            STATS::get_miss.add_value(1, (C::CACHE_NAME,));
            let blob = self.blobstore.get(ctx, key).await?;
            if let Some(ref blob) = blob {
                let key = key.to_owned();
                cloned!(self.cache, blob);
                tokio::spawn(async move { cache.put(&key, blob).await });
            }
            Ok(blob)
        }
    }

    async fn put<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        let can_put = self.take_put_lease(&key).await;
        if can_put {
            self.blobstore.put(ctx, key.clone(), value.clone()).await?;

            cloned!(self.cache, self.lease);
            let cache_put = async move {
                cache.put(&key, value.into()).await;
                lease.release_lease(&key).await
            };
            if self.lazy_cache_put {
                tokio::spawn(cache_put);
            } else {
                let _ = cache_put.await;
            }
        }
        Ok(())
    }

    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        let present = self.cache.check_present(key).await;
        if present {
            STATS::presence_hit.add_value(1, (C::CACHE_NAME,));
            Ok(BlobstoreIsPresent::Present)
        } else {
            STATS::presence_miss.add_value(1, (C::CACHE_NAME,));
            self.blobstore.is_present(ctx, key).await
        }
    }
}

#[async_trait]
impl<C, L, T> CacheBlobstoreExt for CacheBlobstore<C, L, T>
where
    C: CacheOps + Clone + 'static,
    L: LeaseOps + Clone + 'static,
    T: Blobstore,
{
    async fn get_no_cache_fill<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        let blob = self.cache.get(key).await;
        if blob.is_some() {
            Ok(blob)
        } else {
            self.blobstore.get(ctx, key).await
        }
    }

    async fn get_cache_only<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        self.cache.get(key).map(Ok).await
    }
}

impl<C, L, T> fmt::Debug for CacheBlobstore<C, L, T>
where
    C: CacheOps + Clone,
    L: LeaseOps + Clone,
    T: Blobstore,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CacheBlobstore")
            .field("blobstore", &self.blobstore)
            .field("cache", &self.cache)
            .field("lease", &self.lease)
            .finish()
    }
}

#[async_trait]
impl<T: CacheBlobstoreExt> CacheBlobstoreExt for CountedBlobstore<T> {
    #[inline]
    async fn get_no_cache_fill<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        self.as_inner().get_no_cache_fill(ctx, key).await
    }

    #[inline]
    async fn get_cache_only<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        self.as_inner().get_cache_only(ctx, key).await
    }
}

#[async_trait]
impl<T: CacheBlobstoreExt> CacheBlobstoreExt for PrefixBlobstore<T> {
    #[inline]
    async fn get_no_cache_fill<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        self.as_inner()
            .get_no_cache_fill(ctx, &self.prepend(key))
            .await
    }

    #[inline]
    async fn get_cache_only<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        self.as_inner()
            .get_cache_only(ctx, &self.prepend(key))
            .await
    }
}

#[async_trait]
impl<T: CacheBlobstoreExt> CacheBlobstoreExt for RedactedBlobstore<T> {
    #[inline]
    async fn get_no_cache_fill<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        let blobstore = self.access_blobstore(ctx, key, GET_OPERATION)?;
        blobstore.get_no_cache_fill(ctx, key).await
    }

    #[inline]
    async fn get_cache_only<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        let blobstore = self.access_blobstore(ctx, key, GET_OPERATION)?;
        blobstore.get_cache_only(ctx, key).await
    }
}
