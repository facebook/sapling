/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use abomonation::Abomonation;
use cachelib::{get_cached, set_cached, VolatileLruCachePool};
use mononoke_types::RepositoryId;
use stats::prelude::*;
use std::collections::HashMap;
use std::hash::Hash;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, Ordering};

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

    pub fn populate<K, V, F>(
        &self,
        repo_id: RepositoryId,
        output: &mut HashMap<K, V>,
        pending: Vec<K>,
        cache_key: F,
    ) -> Vec<K>
    where
        F: Fn(RepositoryId, &K) -> CacheKey<V>,
        K: Eq + Hash,
        V: Cacheable + Abomonation + Clone + Send + 'static,
    {
        pending
            .into_iter()
            .filter_map(|key| match self.get(&cache_key(repo_id, &key)) {
                Some(value) => {
                    output.insert(key, value);
                    None
                }
                None => Some(key),
            })
            .collect()
    }

    pub fn tracked<'a>(&'a self) -> TrackedLocalCache<'a> {
        TrackedLocalCache {
            inner: &self,
            filled: AtomicBool::new(false),
        }
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
        let r = set_cached(self.get_cache::<V>(), &key.key, value);
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

pub struct TrackedLocalCache<'a> {
    inner: &'a LocalCache,
    filled: AtomicBool,
}

impl<'a> TrackedLocalCache<'a> {
    pub fn get<V>(&self, key: &CacheKey<V>) -> Option<V>
    where
        V: Cacheable + Abomonation + Clone + Send + 'static,
    {
        self.inner.get(key)
    }

    pub fn fill<V>(&self, key: &CacheKey<V>, value: &V)
    where
        V: Cacheable + Abomonation + Clone + Send + 'static,
    {
        if !self.did_fill() {
            self.filled.store(true, Ordering::Relaxed);
        }

        self.inner.fill(key, value)
    }

    pub fn populate<K, V, F>(
        &self,
        repo_id: RepositoryId,
        output: &mut HashMap<K, V>,
        pending: Vec<K>,
        cache_key: F,
    ) -> Vec<K>
    where
        F: Fn(RepositoryId, &K) -> CacheKey<V>,
        K: Eq + Hash,
        V: Cacheable + Abomonation + Clone + Send + 'static,
    {
        self.inner.populate(repo_id, output, pending, cache_key)
    }

    pub fn did_fill(&self) -> bool {
        self.filled.load(Ordering::Relaxed)
    }

    pub fn untracked(&self) -> &'a LocalCache {
        &self.inner
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
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
