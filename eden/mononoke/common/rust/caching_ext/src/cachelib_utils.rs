/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::hash::Hash;
use std::time::Duration;

use anyhow::Result;
use cachelib::VolatileLruCachePool;
use cachelib::bincode_cache;

use crate::CachelibKey;
use crate::mock_store::MockStore;

#[derive(Clone)]
pub enum CachelibHandler<T> {
    Bincode(VolatileLruCachePool),
    Mock(MockStore<T>),
    Noop,
}

impl<T: bincode::Encode + bincode::Decode<()> + Clone> CachelibHandler<T> {
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
            CachelibHandler::Bincode(cache) => bincode_cache::get_cached(cache, key),
            CachelibHandler::Mock(store) => Ok(store.get(key)),
            CachelibHandler::Noop => Ok(None),
        }
    }

    pub fn set_cached(&self, key: &str, value: &T, ttl: Option<Duration>) -> Result<()> {
        match self {
            CachelibHandler::Bincode(cache) => {
                if justknobs::eval("scm/mononoke:caching_ext_use_set_or_replace", None, None)? {
                    bincode_cache::set_or_replace_cached(cache, key, value, ttl)
                } else {
                    bincode_cache::set_cached(cache, key, value, ttl).map(|_| ())
                }
            }
            CachelibHandler::Mock(store) => {
                store.set(key, value.clone());
                Ok(())
            }
            CachelibHandler::Noop => Ok(()),
        }
    }

    pub fn create_mock() -> Self {
        CachelibHandler::Mock(MockStore::new())
    }

    pub fn create_noop() -> Self {
        CachelibHandler::Noop
    }

    #[cfg(test)]
    pub(crate) fn gets_count(&self) -> usize {
        use std::sync::atomic::Ordering;
        match self {
            CachelibHandler::Bincode(_) | CachelibHandler::Noop => unimplemented!(),
            CachelibHandler::Mock(MockStore { get_count, .. }) => get_count.load(Ordering::SeqCst),
        }
    }

    pub fn mock_store(&self) -> Option<&MockStore<T>> {
        match self {
            CachelibHandler::Bincode(_) | CachelibHandler::Noop => None,
            CachelibHandler::Mock(mock) => Some(mock),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use quickcheck::TestResult;
    use quickcheck::quickcheck;

    use super::*;

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
                if initial_keys.contains_key(key) {
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
            let mut fetched: HashSet<_> = fetched.into_keys().collect();

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
