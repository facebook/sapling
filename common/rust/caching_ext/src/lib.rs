/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]
#![feature(never_type)]

mod cachelib_utils;
mod memcache_utils;
mod mock_store;

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::Arc;

use bytes::Bytes;
use cachelib::Abomonation;
use cloned::cloned;
use failure_ext::prelude::*;
use futures::{
    future::{join_all, ok},
    prelude::*,
};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use iobuf::IOBuf;
use memcache::{KeyGen, MEMCACHE_VALUE_MAX_SIZE};
use mononoke_types::RepositoryId;

pub use crate::cachelib_utils::CachelibHandler;
pub use crate::memcache_utils::MemcacheHandler;
pub use crate::mock_store::MockStoreStats;

/// Error type to help with proper reporting of memcache errors
pub enum McErrorKind {
    /// error came from calling memcache API
    MemcacheInternal,
    /// value returned from memcache was None
    Missing,
    /// deserialization of memcache data to Rust structures failed
    Deserialization,
}

pub type McResult<T> = ::std::result::Result<T, McErrorKind>;

struct CachelibKey(String);
struct MemcacheKey(String);

pub struct GetOrFillMultipleFromCacheLayers<Key, T> {
    pub repo_id: RepositoryId,
    pub get_cache_key: Arc<dyn Fn(RepositoryId, &Key) -> String>,
    pub cachelib: CachelibHandler<T>,
    pub keygen: KeyGen,
    pub memcache: MemcacheHandler,
    pub deserialize: Arc<dyn Fn(IOBuf) -> ::std::result::Result<T, ()> + Send + Sync + 'static>,
    pub serialize: Arc<dyn Fn(&T) -> Bytes + Send + Sync + 'static>,
    pub report_mc_result: Arc<dyn Fn(McResult<()>) + Send + Sync + 'static>,
    pub get_from_db:
        Arc<dyn Fn(HashSet<Key>) -> BoxFuture<HashMap<Key, T>, Error> + Send + Sync + 'static>,
}

impl<Key, T> GetOrFillMultipleFromCacheLayers<Key, T>
where
    Key: Clone + Eq + Hash + Send + 'static,
    T: Abomonation + Clone + Send + 'static,
{
    pub fn run(&self, keys: HashSet<Key>) -> BoxFuture<HashMap<Key, T>, Error> {
        let keys: Vec<(Key, CachelibKey)> = keys
            .into_iter()
            .map(|key| {
                let cache_key = CachelibKey((self.get_cache_key)(self.repo_id, &key));
                (key, cache_key)
            })
            .collect();

        let (fetched_from_cachelib, left_to_fetch) =
            try_boxfuture!(self.cachelib.get_multiple_from_cachelib(keys));

        cloned!(
            self.cachelib,
            self.get_from_db,
            self.memcache,
            self.serialize
        );
        get_multiple_from_memcache(
            left_to_fetch,
            &self.keygen,
            &self.memcache,
            self.deserialize.clone(),
            self.report_mc_result.clone(),
        )
        .then(move |result: ::std::result::Result<_, !>| {
            let (fetched_from_memcache, left_to_fetch) = match result {
                Ok(result) => result,
                Err(never) => never,
            };
            let fetched_from_memcache = cachelib.fill_multiple_cachelib(fetched_from_memcache);

            let mut key_mapping = HashMap::new();
            let left_to_fetch: HashSet<Key> = left_to_fetch
                .into_iter()
                .map(|(key, cache_key, memcache_key)| {
                    key_mapping.insert(key.clone(), (cache_key, memcache_key));
                    key
                })
                .collect();

            // Skip calling get_from_db if we have nothing left to fetch: unlike the Memcache and
            // Cachelib paths, we don't control what this function does, so we have no guarantees
            // that it won't e.g. make a query or increment monitoring counters.
            let fetched_from_db = if left_to_fetch.is_empty() {
                ok(HashMap::new()).left_future()
            } else {
                get_from_db(left_to_fetch).right_future()
            };

            fetched_from_db.map(move |fetched_from_db| {
                let fetched_from_db: HashMap<Key, (T, CachelibKey, MemcacheKey)> = fetched_from_db
                    .into_iter()
                    .map(move |(key, value)| {
                        let (cache_key, memcache_key) = key_mapping.remove(&key).expect(
                            "caching_ext: Missing entry in key_mapping, this should not happen",
                        );
                        (key, (value, cache_key, memcache_key))
                    })
                    .collect();

                let fetched_from_db = cachelib.fill_multiple_cachelib(fill_multiple_memcache(
                    fetched_from_db,
                    memcache,
                    serialize,
                ));

                let mut fetched = HashMap::new();
                fetched.extend(fetched_from_cachelib);
                fetched.extend(fetched_from_memcache);
                fetched.extend(fetched_from_db);
                fetched
            })
        })
        .boxify()
    }
}

fn get_multiple_from_memcache<Key, T>(
    keys: Vec<(Key, CachelibKey)>,
    keygen: &KeyGen,
    memcache: &MemcacheHandler,
    deserialize: Arc<dyn Fn(IOBuf) -> ::std::result::Result<T, ()> + Send + Sync + 'static>,
    report_mc_result: Arc<dyn Fn(McResult<()>) + Send + Sync + 'static>,
) -> impl Future<
    Item = (
        HashMap<Key, (T, CachelibKey)>,
        Vec<(Key, CachelibKey, MemcacheKey)>,
    ),
    Error = !,
>
where
    Key: Eq + Hash,
{
    let mc_fetch_futs: Vec<_> = keys
        .into_iter()
        .map(move |(key, cache_key)| {
            let memcache_key = MemcacheKey(keygen.key(&cache_key.0));
            memcache
                .get(memcache_key.0.clone())
                .map_err(|()| McErrorKind::MemcacheInternal)
                .and_then(|maybe_serialized| maybe_serialized.ok_or(McErrorKind::Missing))
                .then(move |result| -> ::std::result::Result<_, !> {
                    Ok((key, result, cache_key, memcache_key))
                })
        })
        .collect();

    join_all(mc_fetch_futs).map(move |entries| {
        let mut fetched = HashMap::new();
        let mut left_to_fetch = Vec::new();

        for (key, entry_result, cache_key, memcache_key) in entries {
            let entry_result = entry_result.and_then(|serialized| {
                deserialize(serialized).map_err(|()| McErrorKind::Deserialization)
            });

            match entry_result {
                Ok(entry) => {
                    report_mc_result(Ok(()));
                    fetched.insert(key, (entry, cache_key));
                }
                Err(err) => {
                    report_mc_result(Err(err));
                    left_to_fetch.push((key, cache_key, memcache_key))
                }
            }
        }

        (fetched, left_to_fetch)
    })
}

fn fill_multiple_memcache<Key, T>(
    keys: HashMap<Key, (T, CachelibKey, MemcacheKey)>,
    memcache: MemcacheHandler,
    serialize: Arc<dyn Fn(&T) -> Bytes + Send + Sync + 'static>,
) -> HashMap<Key, (T, CachelibKey)>
where
    Key: Eq + Hash,
{
    keys.into_iter()
        .map(|(key, (value, cache_key, memcache_key))| {
            let serialized = serialize(&value);

            if serialized.len() < MEMCACHE_VALUE_MAX_SIZE {
                ::tokio::spawn(memcache.set(memcache_key.0, serialized));
            }

            (key, (value, cache_key))
        })
        .collect()
}

#[cfg(test)]
mod test {
    use super::*;

    use futures::future::ok;
    use maplit::{hashmap, hashset};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    fn test_cachelib_cachekey(_repoid: RepositoryId, key: &String) -> String {
        key.clone()
    }

    fn test_report_mc_result(_: McResult<()>) {}

    fn create_params(
        cachelib: CachelibHandler<u8>,
        memcache: MemcacheHandler,
        db_data_calls: Arc<AtomicUsize>,
        db_data_fetches: Arc<AtomicUsize>,
        db_data: HashMap<String, u8>,
    ) -> GetOrFillMultipleFromCacheLayers<String, u8> {
        let deserialize = |_buf: IOBuf| -> ::std::result::Result<u8, ()> { Ok(0) };

        let serialize = |byte: &u8| -> Bytes { Bytes::from(vec![byte.clone()]) };
        let get_from_db = move |keys: HashSet<String>| -> BoxFuture<HashMap<String, u8>, Error> {
            db_data_calls.fetch_add(1, Ordering::SeqCst);
            db_data_fetches.fetch_add(keys.len(), Ordering::SeqCst);
            let mut res = HashMap::new();
            for key in keys {
                if let Some(value) = db_data.get(&key) {
                    res.insert(key, *value);
                }
            }
            ok(res).boxify()
        };
        GetOrFillMultipleFromCacheLayers {
            repo_id: RepositoryId::new(0),
            get_cache_key: Arc::new(test_cachelib_cachekey),
            keygen: KeyGen::new("", 0, 0),
            cachelib,
            memcache,
            deserialize: Arc::new(deserialize),
            serialize: Arc::new(serialize),
            report_mc_result: Arc::new(test_report_mc_result),
            get_from_db: Arc::new(get_from_db),
        }
    }

    #[test]
    fn simple() {
        let db_data = hashmap! {};
        let db_data_fetches = Arc::new(AtomicUsize::new(0));
        let cachelib = CachelibHandler::create_mock();
        let memcache = MemcacheHandler::create_mock();

        let params = create_params(
            cachelib.clone(),
            memcache.clone(),
            Arc::new(AtomicUsize::new(0)),
            db_data_fetches.clone(),
            db_data,
        );

        let mut runtime = tokio::runtime::Runtime::new().unwrap();
        let f = params.run(hashset! {});
        let res = runtime.block_on(f).unwrap();
        assert_eq!(res.len(), 0);
        assert_eq!(cachelib.gets_count(), 0);
        assert_eq!(memcache.gets_count(), 0);

        let f = params.run(hashset! {"key".to_string()});
        let res = runtime.block_on(f).unwrap();
        assert_eq!(res.len(), 0);
        assert_eq!(cachelib.gets_count(), 1);
        assert_eq!(memcache.gets_count(), 1);
        assert_eq!(db_data_fetches.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn fetch_from_db_cachelib_memcache() {
        let db_data = hashmap! {"key".to_string() => 0};
        let db_data_fetches = Arc::new(AtomicUsize::new(0));
        let cachelib = CachelibHandler::create_mock();
        let memcache = MemcacheHandler::create_mock();

        let mut params = create_params(
            cachelib.clone(),
            memcache.clone(),
            Arc::new(AtomicUsize::new(0)),
            db_data_fetches.clone(),
            db_data,
        );

        // Fetch from db
        let f = params.run(hashset! {"key".to_string()});
        let mut runtime = tokio::runtime::Runtime::new().unwrap();
        let res = runtime.block_on(f).unwrap();
        assert_eq!(res.len(), 1);

        assert_eq!(cachelib.gets_count(), 1);
        assert_eq!(memcache.gets_count(), 1);
        assert_eq!(db_data_fetches.load(Ordering::Relaxed), 1);

        // Now fetch from cachelib
        let f = params.run(hashset! {"key".to_string()});
        let res = runtime.block_on(f).unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(cachelib.gets_count(), 2);
        assert_eq!(memcache.gets_count(), 1);
        assert_eq!(db_data_fetches.load(Ordering::Relaxed), 1);

        // Reset cachelib, fetch from memcache
        let cachelib = CachelibHandler::create_mock();
        params.cachelib = cachelib.clone();
        let f = params.run(hashset! {"key".to_string()});
        let res = runtime.block_on(f).unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(cachelib.gets_count(), 1);
        assert_eq!(memcache.gets_count(), 2);
        assert_eq!(db_data_fetches.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn fetch_from_db() {
        let db_data = hashmap! {
            "key0".to_string() => 0,
            "key1".to_string() => 1,
            "key2".to_string() => 2,
        };
        let db_data_fetches = Arc::new(AtomicUsize::new(0));
        let cachelib = CachelibHandler::create_mock();
        let memcache = MemcacheHandler::create_mock();

        let params = create_params(
            cachelib.clone(),
            memcache.clone(),
            Arc::new(AtomicUsize::new(0)),
            db_data_fetches.clone(),
            db_data,
        );
        let mut runtime = tokio::runtime::Runtime::new().unwrap();
        let f = params.run(hashset! {
            "key0".to_string(), "key1".to_string(), "key2".to_string()
        });
        runtime.block_on(f).unwrap();

        assert_eq!(cachelib.gets_count(), 3);
        assert_eq!(memcache.gets_count(), 3);
        assert_eq!(db_data_fetches.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn fetch_from_all() {
        let db_data = hashmap! {
            "key0".to_string() => 0,
            "key1".to_string() => 1,
            "key2".to_string() => 2,
        };

        let db_data_fetches = Arc::new(AtomicUsize::new(0));
        let cachelib = CachelibHandler::create_mock();
        let memcache = MemcacheHandler::create_mock();

        let mut params = create_params(
            cachelib.clone(),
            memcache.clone(),
            Arc::new(AtomicUsize::new(0)),
            db_data_fetches.clone(),
            db_data,
        );

        let mut runtime = tokio::runtime::Runtime::new().unwrap();
        let f = params.run(hashset! {"key1".to_string()});
        runtime.block_on(f).unwrap();
        assert_eq!(cachelib.gets_count(), 1);
        assert_eq!(memcache.gets_count(), 1);
        assert_eq!(db_data_fetches.load(Ordering::Relaxed), 1);

        // Reset cachelib
        let cachelib = CachelibHandler::create_mock();
        params.cachelib = cachelib.clone();
        let f = params.run(hashset! {"key0".to_string()});
        runtime.block_on(f).unwrap();
        assert_eq!(cachelib.gets_count(), 1);
        assert_eq!(memcache.gets_count(), 2);
        assert_eq!(db_data_fetches.load(Ordering::Relaxed), 2);

        let f = params.run(hashset! {
            "key0".to_string(), "key1".to_string(), "key2".to_string()
        });
        let res = runtime.block_on(f).unwrap();
        assert_eq!(res.len(), 3);
        assert_eq!(cachelib.gets_count(), 1 + 3); // 3 new fetches from cachelib, 2 misses
        assert_eq!(memcache.gets_count(), 2 + 2); // 2 new fetches from memcache, 1 miss
        assert_eq!(db_data_fetches.load(Ordering::Relaxed), 2 + 1); // 1 fetch from db

        // // Only from cachelib
        let f = params.run(hashset! {
            "key0".to_string(), "key1".to_string(), "key2".to_string()
        });
        let res = runtime.block_on(f).unwrap();
        assert_eq!(res.len(), 3);
        assert_eq!(cachelib.gets_count(), 7);
        assert_eq!(memcache.gets_count(), 4);
        assert_eq!(db_data_fetches.load(Ordering::Relaxed), 3);

        // // Reset cachelib, only from memcache
        let cachelib = CachelibHandler::create_mock();
        params.cachelib = cachelib.clone();
        let f = params.run(hashset! {
            "key0".to_string(), "key1".to_string(), "key2".to_string()
        });
        let res = runtime.block_on(f).unwrap();
        assert_eq!(res.len(), 3);
        assert_eq!(cachelib.gets_count(), 3); // 3 misses
        assert_eq!(memcache.gets_count(), 4 + 3); // 3 hits
        assert_eq!(db_data_fetches.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn get_from_db_elision() {
        let db_data = hashmap! {};
        let db_data_calls = Arc::new(AtomicUsize::new(0));
        let cachelib = CachelibHandler::create_mock();
        let memcache = MemcacheHandler::create_mock();

        let params = create_params(
            cachelib.clone(),
            memcache.clone(),
            db_data_calls.clone(),
            Arc::new(AtomicUsize::new(0)),
            db_data,
        );

        let mut runtime = tokio::runtime::Runtime::new().unwrap();

        let f = params.run(hashset! {});
        let _ = runtime.block_on(f).unwrap();
        assert_eq!(db_data_calls.load(Ordering::SeqCst), 0);
    }
}
