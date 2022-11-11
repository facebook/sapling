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

/// SQL query config.
#[facet::facet]
pub struct SqlQueryConfig {
    pub keygen: KeyGen,
    pub memcache: MemcacheHandler,
    pub cache_pool: VolatileLruCachePool,
}
