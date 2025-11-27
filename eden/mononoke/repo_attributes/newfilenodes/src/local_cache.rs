/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::marker::PhantomData;

use caching_ext::CacheHandlerFactory;
use caching_ext::CachelibHandler;
use filenodes::FilenodeInfo;
use filenodes::FilenodeRange;
use stats::prelude::*;

define_stats! {
    prefix = "mononoke.filenodes";
    fill_cache_fail: timeseries(Sum),
}

#[derive(Clone)]
pub struct CacheKey<V> {
    pub key: String,
    /// value is used to enforce that a CacheKey for a given type V can only be used to fetch
    /// values of type V.
    pub value: PhantomData<V>,
}

pub struct LocalCache {
    filenode_cache: CachelibHandler<FilenodeInfo>,
    history_cache: CachelibHandler<FilenodeRange>,
}

impl LocalCache {
    pub fn new(
        filenode_cache_handler_factory: &CacheHandlerFactory,
        history_cache_handler_factory: &CacheHandlerFactory,
    ) -> Self {
        LocalCache {
            filenode_cache: filenode_cache_handler_factory.cachelib(),
            history_cache: history_cache_handler_factory.cachelib(),
        }
    }

    pub fn new_noop() -> Self {
        Self::new(&CacheHandlerFactory::Noop, &CacheHandlerFactory::Noop)
    }

    #[cfg(test)]
    pub fn new_mock() -> Self {
        Self::new(&CacheHandlerFactory::Mocked, &CacheHandlerFactory::Mocked)
    }

    pub fn get_filenode(&self, key: &CacheKey<FilenodeInfo>) -> Option<FilenodeInfo> {
        match self.filenode_cache.get_cached(&key.key) {
            Ok(Some(r)) => Some(r),
            _ => None,
        }
    }

    pub fn fill_filenode(&self, key: &CacheKey<FilenodeInfo>, value: &FilenodeInfo) {
        let r = self.filenode_cache.set_cached(&key.key, value, None);
        if r.is_err() {
            STATS::fill_cache_fail.add_value(1);
        }
    }

    pub fn get_history(&self, key: &CacheKey<FilenodeRange>) -> Option<FilenodeRange> {
        match self.history_cache.get_cached(&key.key) {
            Ok(Some(r)) => Some(r),
            _ => None,
        }
    }

    pub fn fill_history(&self, key: &CacheKey<FilenodeRange>, value: &FilenodeRange) {
        let r = self.history_cache.set_cached(&key.key, value, None);
        if r.is_err() {
            STATS::fill_cache_fail.add_value(1);
        }
    }
}
