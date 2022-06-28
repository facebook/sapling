/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use abomonation::Abomonation;
use cachelib::get_cached;
use cachelib::set_cached;
use cachelib::VolatileLruCachePool;
use stats::prelude::*;
use std::marker::PhantomData;

define_stats! {
    prefix = "mononoke.filenodes";
    fill_cache_fail: timeseries(Sum),
}

#[derive(Copy, Clone)]
pub enum CachePool {
    Filenodes,
    FilenodesHistory,
}

pub trait Cacheable {
    const POOL: CachePool;
}

#[derive(Clone)]
pub struct CacheKey<V> {
    pub key: String,
    /// value is used to enforce that a CacheKey for a given type V can only be used to fetch
    /// values of type V.
    pub value: PhantomData<V>,
}

pub enum LocalCache {
    Cachelib(CachelibCache),
    Noop,
    #[cfg(test)]
    Test(self::test::HashMapCache),
}

impl LocalCache {
    pub fn get<V>(&self, key: &CacheKey<V>) -> Option<V>
    where
        V: Cacheable + Abomonation + Clone + Send + 'static,
    {
        match self {
            Self::Cachelib(cache) => cache.get(key),
            Self::Noop => None,
            #[cfg(test)]
            Self::Test(cache) => cache.get(key),
        }
    }

    pub fn fill<V>(&self, key: &CacheKey<V>, value: &V)
    where
        V: Cacheable + Abomonation + Clone + Send + 'static,
    {
        match self {
            Self::Cachelib(cache) => cache.fill(key, value),
            Self::Noop => {}
            #[cfg(test)]
            Self::Test(cache) => cache.fill(key, value),
        };
    }
}

pub struct CachelibCache {
    filenodes_cache_pool: VolatileLruCachePool,
    filenodes_history_cache_pool: VolatileLruCachePool,
}

impl CachelibCache {
    pub fn new(
        filenodes_cache_pool: VolatileLruCachePool,
        filenodes_history_cache_pool: VolatileLruCachePool,
    ) -> Self {
        Self {
            filenodes_cache_pool,
            filenodes_history_cache_pool,
        }
    }

    fn get<V>(&self, key: &CacheKey<V>) -> Option<V>
    where
        V: Cacheable + Abomonation + Clone + Send + 'static,
    {
        match get_cached(self.get_cache::<V>(), &key.key) {
            Ok(Some(r)) => Some(r),
            _ => None,
        }
    }

    fn fill<V>(&self, key: &CacheKey<V>, value: &V)
    where
        V: Cacheable + Abomonation + Clone + Send + 'static,
    {
        let r = set_cached(self.get_cache::<V>(), &key.key, value, None);
        if r.is_err() {
            STATS::fill_cache_fail.add_value(1);
        }
    }

    fn get_cache<V>(&self) -> &VolatileLruCachePool
    where
        V: Cacheable,
    {
        match V::POOL {
            CachePool::Filenodes => &self.filenodes_cache_pool,
            CachePool::FilenodesHistory => &self.filenodes_history_cache_pool,
        }
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    pub struct HashMapCache {
        hashmap: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl HashMapCache {
        pub fn new() -> Self {
            Self {
                hashmap: Mutex::new(HashMap::new()),
            }
        }

        pub fn get<V>(&self, key: &CacheKey<V>) -> Option<V>
        where
            V: Cacheable + Abomonation + Clone + Send + 'static,
        {
            let mut bytes = match self.hashmap.lock().unwrap().get(&key.key) {
                Some(obj) => obj.clone(),
                None => {
                    return None;
                }
            };
            let (obj, tail) = unsafe { abomonation::decode::<V>(&mut bytes) }.unwrap();
            assert!(tail.is_empty());
            Some(obj.clone())
        }

        pub fn fill<V>(&self, key: &CacheKey<V>, value: &V)
        where
            V: Cacheable + Abomonation + Clone + Send + 'static,
        {
            let mut bytes = Vec::new();
            unsafe { abomonation::encode(value, &mut bytes) }.unwrap();
            self.hashmap.lock().unwrap().insert(key.key.clone(), bytes);
        }
    }
}
