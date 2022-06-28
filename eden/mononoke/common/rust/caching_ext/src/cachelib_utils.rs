/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::mock_store::MockStore;
use anyhow::Result;
use cachelib::get_cached;
use cachelib::set_cached;
use cachelib::Abomonation;
use cachelib::VolatileLruCachePool;

use crate::CachelibKey;

#[derive(Clone)]
pub enum CachelibHandler<T> {
    Real(VolatileLruCachePool),
    #[allow(dead_code)]
    Mock(MockStore<T>),
}

impl<T> From<VolatileLruCachePool> for CachelibHandler<T> {
    fn from(cache: VolatileLruCachePool) -> Self {
        CachelibHandler::Real(cache)
    }
}

impl<T: Abomonation + Clone + Send + 'static> CachelibHandler<T> {
    pub(crate) fn get_multiple_from_cachelib<Key: Eq + Hash>(
        &self,
        keys: Vec<(Key, CachelibKey)>,
    ) -> Result<(HashMap<Key, T>, Vec<(Key, CachelibKey)>)> {
        let mut fetched = HashMap::new();
        let mut left_to_fetch = Vec::new();

        for (key, cache_key) in keys {
            match self.get_cached(&cache_key.0)? {
                Some(value) => {
                    fetched.insert(key, value);
                }
                None => {
                    left_to_fetch.push((key, cache_key));
                }
            }
        }

        Ok((fetched, left_to_fetch))
    }

    pub fn get_cached(&self, key: &String) -> Result<Option<T>> {
        match self {
            CachelibHandler::Real(ref cache) => get_cached(cache, key),
            CachelibHandler::Mock(store) => Ok(store.get(key)),
        }
    }

    pub fn set_cached(&self, key: &String, value: &T, ttl: Option<Duration>) -> Result<bool> {
        match self {
            CachelibHandler::Real(ref cache) => set_cached(cache, key, value, ttl),
            CachelibHandler::Mock(store) => {
                store.set(key, value.clone());
                Ok(true)
            }
        }
    }

    #[allow(dead_code)]
    pub fn create_mock() -> Self {
        CachelibHandler::Mock(MockStore::new())
    }

    #[allow(dead_code)]
    pub(crate) fn gets_count(&self) -> usize {
        match self {
            CachelibHandler::Real(_) => unimplemented!(),
            CachelibHandler::Mock(MockStore { ref get_count, .. }) => {
                get_count.load(Ordering::SeqCst)
            }
        }
    }

    pub fn mock_store(&self) -> Option<&MockStore<T>> {
        match self {
            CachelibHandler::Real(_) => None,
            CachelibHandler::Mock(ref mock) => Some(mock),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashSet;

    use quickcheck::quickcheck;
    use quickcheck::TestResult;

    quickcheck! {
        fn multiple_roundtrip(
            initial_keys: HashMap<String, String>,
            keys_to_query: HashSet<String>
        ) -> TestResult {
            let get_query = keys_to_query.clone().into_iter().map(|key| (key.clone(),  CachelibKey(key))).collect();

            let mock_cachelib = MockStore::new();
            let cachelib_handler = CachelibHandler::Mock(mock_cachelib.clone());

            for (k, v) in initial_keys.iter() {
                let _ = cachelib_handler.set_cached(k, v, None);
            }

            if mock_cachelib.data() != initial_keys {
                return TestResult::error("After fill the content of cache is incorrect");
            }

            let (fetched, left) = cachelib_handler.get_multiple_from_cachelib(get_query).unwrap();

            for (key, cache_key) in &left {
                if key != &cache_key.0 {
                    return TestResult::error("Key and cache key got mixed in left!");
                }
                if initial_keys.get(key).is_some() {
                    return TestResult::error("After get_multiple_from_cachelib left is incorrect");
                }
            }

            for (key, val) in fetched.iter() {
                if initial_keys.get(key) != Some(val) {
                    return TestResult::error("Wrong value returned from get");
                }
            }

            if fetched.len() + left.len() != keys_to_query.len() {
                return TestResult::error("Returned wrong number of items from get");
            }

            let left: HashSet<_> = left.into_iter().map(|(key, _)| key).collect();
            let mut fetched: HashSet<_> = fetched.into_iter().map(|(key, _)| key).collect();

            if fetched.len() + left.len() != keys_to_query.len() {
                return TestResult::error("Returned wrong number of unique items from get");
            }

            fetched.extend(left);

            if fetched != keys_to_query {
                return TestResult::error("Didn't return all keys from get");
            }

            TestResult::passed()
        }
    }
}
