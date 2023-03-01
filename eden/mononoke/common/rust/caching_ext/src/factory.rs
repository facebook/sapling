/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use abomonation::Abomonation;
use cachelib::VolatileLruCachePool;
use memcache::MemcacheClient;

use crate::CachelibHandler;
use crate::MemcacheHandler;

/// Builder to construct caches, depending on the desired caching mode.
pub enum CacheHandlerFactory {
    /// Caching is via a local cache (cachelib) and a shared cache (memcache).
    Shared {
        /// The cachelib pool to use for local caching.
        cachelib_pool: VolatileLruCachePool,

        /// The memcache client to use for shared caching.
        memcache_client: MemcacheClient,
    },

    /// Caching is via a local cache (cachelib) only.
    Local { cachelib_pool: VolatileLruCachePool },

    /// Caching is mocked for testing purposes, with items cached in an
    /// in-memory store.
    Mocked,

    /// Caching is always a no-op.
    Noop,
}

impl CacheHandlerFactory {
    /// Build cachelib cache handler.
    pub fn cachelib<T>(&self) -> CachelibHandler<T>
    where
        T: Abomonation + Send + Clone + 'static,
    {
        match self {
            Self::Shared { cachelib_pool, .. } | Self::Local { cachelib_pool, .. } => {
                cachelib_pool.clone().into()
            }
            Self::Mocked => CachelibHandler::create_mock(),
            Self::Noop => CachelibHandler::create_noop(),
        }
    }

    /// Build memcache cache handler.
    pub fn memcache(&self) -> MemcacheHandler {
        match self {
            Self::Shared {
                memcache_client, ..
            } => memcache_client.clone().into(),
            Self::Mocked => MemcacheHandler::create_mock(),
            Self::Noop | Self::Local { .. } => MemcacheHandler::create_noop(),
        }
    }
}
