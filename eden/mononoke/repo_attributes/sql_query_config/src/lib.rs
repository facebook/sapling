/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! SQL query config.
//!
//! Retries and caching option for generic SQL queries.

use caching_ext::CacheHandlerFactory;
use memcache::KeyGen;

pub struct CachingConfig {
    pub keygen: KeyGen,
    pub cache_handler_factory: CacheHandlerFactory,
}

/// SQL query config.
#[facet::facet]
pub struct SqlQueryConfig {
    pub caching: Option<CachingConfig>,
}
