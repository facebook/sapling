/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! SQL query config.
//!
//! Retries and caching option for generic SQL queries.

use cachelib::VolatileLruCachePool;
use caching_ext::MemcacheHandler;
use memcache::KeyGen;

pub struct CachingConfig {
    pub keygen: KeyGen,
    pub memcache: MemcacheHandler,
    pub cache_pool: VolatileLruCachePool,
}

/// SQL query config.
#[facet::facet]
pub struct SqlQueryConfig {
    pub caching: Option<CachingConfig>,
}
