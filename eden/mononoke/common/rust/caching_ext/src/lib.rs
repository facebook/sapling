/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]
#![feature(never_type)]

mod cachelib_utils;
mod memcache_utils;
mod mock_store;

use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::{sync::Arc, time::Duration};

use abomonation::Abomonation;
use anyhow::{Context as _, Error};
use async_trait::async_trait;
use auto_impl::auto_impl;
use bytes::Bytes;
use cloned::cloned;
use futures::{compat::Future01CompatExt, future};
use futures_01_ext::{try_boxfuture, BoxFuture, FutureExt as _};
use futures_old::{
    future::{join_all, ok},
    prelude::*,
};
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

pub type McResult<T> = Result<T, McErrorKind>;

struct CachelibKey(String);
struct MemcacheKey(String);

pub type CachingDeterminator<T> = fn(&T) -> CacheDisposition;

pub fn cache_all_determinator<T>(_value: &T) -> CacheDisposition {
    CacheDisposition::Cache
}

pub enum CacheDisposition {
    Cache,
    CacheWithTtl(Duration),
    Ignore,
}

#[derive(Copy, Clone)]
pub enum CacheTtl {
    NoTtl,
    Ttl(Duration),
}

#[derive(Copy, Clone)]
pub enum CacheDispositionNew {
    Cache(CacheTtl),
    Ignore,
}

pub trait MemcacheEntity: Sized {
    fn serialize(&self) -> Bytes;

    fn deserialize(bytes: Bytes) -> Result<Self, ()>;

    fn report_mc_result(res: &McResult<Self>); // TODO: Default impl here
}

#[auto_impl(&)]
pub trait EntityStore<V> {
    fn cachelib(&self) -> &CachelibHandler<V>;

    fn keygen(&self) -> &KeyGen;

    fn memcache(&self) -> &MemcacheHandler;

    fn cache_determinator(&self, v: &V) -> CacheDispositionNew;

    /// Whether Memcache writes should run in the background. This is normally the desired behavior
    /// so this defaults to true, but for tests it's useful to run them synchronously to get
    /// consistent outcomes.
    fn spawn_memcache_writes(&self) -> bool {
        true
    }
}

#[async_trait]
#[auto_impl(&)]
pub trait KeyedEntityStore<K, V>: EntityStore<V> {
    fn get_cache_key(&self, key: &K) -> String;

    async fn get_from_db(&self, keys: HashSet<K>) -> Result<HashMap<K, V>, Error>;
}

pub async fn get_or_fill<K, V>(
    store: impl KeyedEntityStore<K, V>,
    keys: HashSet<K>,
) -> Result<HashMap<K, V>, Error>
where
    K: Hash + Eq + Clone,
    // TODO: We should relax the bounds on cachelib's set_cached. We don't need all of this:
    V: Abomonation + MemcacheEntity + Send + Clone + 'static,
{
    let mut ret = HashMap::<K, V>::new();

    let cachelib_keys: Vec<_> = keys
        .into_iter()
        .map(|key| {
            let cachelib_key = CachelibKey(store.get_cache_key(&key));
            (key, cachelib_key)
        })
        .collect();

    let (fetched_from_cachelib, to_fetch_from_memcache) = store
        .cachelib()
        .get_multiple_from_cachelib::<K>(cachelib_keys)
        .with_context(|| "Error reading from cachelib")?;

    ret.extend(fetched_from_cachelib);

    let to_fetch_from_memcache: Vec<(K, CachelibKey, MemcacheKey)> = to_fetch_from_memcache
        .into_iter()
        .map(|(key, cachelib_key)| {
            let memcache_key = MemcacheKey(store.keygen().key(&cachelib_key.0));
            (key, cachelib_key, memcache_key)
        })
        .collect();

    let to_fetch_from_store = {
        let (fetched_from_memcache, to_fetch_from_store) =
            get_multiple_from_memcache_new(store.memcache(), to_fetch_from_memcache).await;

        fill_multiple_cachelib(
            store.cachelib(),
            fetched_from_memcache
                .values()
                .filter_map(|(v, k)| match store.cache_determinator(v) {
                    CacheDispositionNew::Cache(ttl) => Some((k, ttl, v)),
                    _ => None,
                }),
        );

        ret.extend(fetched_from_memcache.into_iter().map(|(k, (v, _))| (k, v)));

        to_fetch_from_store
    };

    let mut key_mapping = HashMap::new();
    let to_fetch_from_store: HashSet<K> = to_fetch_from_store
        .into_iter()
        .map(|(key, cachelib_key, memcache_key)| {
            key_mapping.insert(key.clone(), (cachelib_key, memcache_key));
            key
        })
        .collect();

    if !to_fetch_from_store.is_empty() {
        let data = store
            .get_from_db(to_fetch_from_store)
            .await
            .with_context(|| "Error reading from store")?;

        let mut cachelib_keys = Vec::new();
        let mut memcache_keys = Vec::new();

        {
            for (key, v) in data.iter() {
                let (cachelib_key, memcache_key) = key_mapping
                    .remove(&key)
                    .expect("caching_ext: Missing entry in key_mapping, this should not happen");

                let ttl = match store.cache_determinator(v) {
                    CacheDispositionNew::Cache(ttl) => ttl,
                    CacheDispositionNew::Ignore => continue,
                };

                memcache_keys.push((memcache_key, ttl, v));
                cachelib_keys.push((cachelib_key, ttl, v));
            }

            fill_multiple_cachelib(store.cachelib(), cachelib_keys);

            fill_multiple_memcache_new(
                store.memcache(),
                memcache_keys,
                store.spawn_memcache_writes(),
            )
            .await;
        }

        ret.extend(data);
    };

    Ok(ret)
}

async fn get_multiple_from_memcache_new<K, V>(
    memcache: &MemcacheHandler,
    keys: Vec<(K, CachelibKey, MemcacheKey)>,
) -> (
    HashMap<K, (V, CachelibKey)>,
    Vec<(K, CachelibKey, MemcacheKey)>,
)
where
    K: Eq + Hash,
    V: MemcacheEntity,
{
    let mc_fetch_futs: Vec<_> = keys
        .into_iter()
        .map(move |(key, cachelib_key, memcache_key)| {
            cloned!(memcache);
            async move {
                let res = memcache
                    .get(memcache_key.0.clone())
                    .compat()
                    .await
                    .map_err(|()| McErrorKind::MemcacheInternal)
                    .and_then(|maybe_bytes| maybe_bytes.ok_or(McErrorKind::Missing))
                    .and_then(|bytes| {
                        V::deserialize(bytes).map_err(|()| McErrorKind::Deserialization)
                    });

                (key, cachelib_key, memcache_key, res)
            }
        })
        .collect();

    let entries = future::join_all(mc_fetch_futs).await;

    let mut fetched = HashMap::new();
    let mut left_to_fetch = Vec::new();

    for (key, cachelib_key, memcache_key, res) in entries {
        V::report_mc_result(&res);

        match res {
            Ok(entity) => {
                fetched.insert(key, (entity, cachelib_key));
            }
            Err(..) => {
                left_to_fetch.push((key, cachelib_key, memcache_key));
            }
        }
    }

    (fetched, left_to_fetch)
}

fn fill_multiple_cachelib<'a, V>(
    cachelib: &'a CachelibHandler<V>,
    data: impl IntoIterator<Item = (impl Borrow<CachelibKey> + 'a, CacheTtl, &'a V)>,
) where
    V: Abomonation + Clone + Send + 'static,
{
    for (cachelib_key, ttl, v) in data {
        let cachelib_key = cachelib_key.borrow();

        match ttl {
            CacheTtl::NoTtl => {
                // NOTE: We ignore failures to cache individual entries here.
                let _ = cachelib.set_cached(&cachelib_key.0, v);
            }
            CacheTtl::Ttl(..) => {
                // Not implemented yet for our cachelib cache.
            }
        }
    }
}

async fn fill_multiple_memcache_new<'a, V: 'a>(
    memcache: &'a MemcacheHandler,
    data: impl IntoIterator<Item = (MemcacheKey, CacheTtl, &'a V)>,
    spawn: bool,
) where
    V: MemcacheEntity,
{
    let futs = data.into_iter().filter_map(|(memcache_key, ttl, v)| {
        let bytes = v.serialize();

        if bytes.len() >= MEMCACHE_VALUE_MAX_SIZE {
            return None;
        }

        cloned!(memcache);

        Some(async move {
            match ttl {
                CacheTtl::NoTtl => {
                    memcache.set(memcache_key.0, bytes).compat().await?;
                }
                CacheTtl::Ttl(ttl) => {
                    memcache
                        .set_with_ttl(memcache_key.0, bytes, ttl)
                        .compat()
                        .await?;
                }
            }

            Result::<_, ()>::Ok(())
        })
    });

    let fut = future::join_all(futs);

    if spawn {
        tokio::task::spawn(fut);
    } else {
        fut.await;
    }
}

pub struct GetOrFillMultipleFromCacheLayers<Key, T> {
    pub repo_id: RepositoryId,
    pub get_cache_key: Arc<dyn Fn(RepositoryId, &Key) -> String + Send + Sync + 'static>,
    pub cachelib: CachelibHandler<T>,
    pub keygen: KeyGen,
    pub memcache: MemcacheHandler,
    pub deserialize: Arc<dyn Fn(Bytes) -> Result<T, ()> + Send + Sync + 'static>,
    pub serialize: Arc<dyn Fn(&T) -> Bytes + Send + Sync + 'static>,
    pub report_mc_result: Arc<dyn Fn(McResult<()>) + Send + Sync + 'static>,
    pub get_from_db:
        Arc<dyn Fn(HashSet<Key>) -> BoxFuture<HashMap<Key, T>, Error> + Send + Sync + 'static>,
    pub determinator: CachingDeterminator<T>,
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
                let cache_key = self.get_cachelib_key(&key);
                (key, cache_key)
            })
            .collect();

        let (fetched_from_cachelib, left_to_fetch) =
            try_boxfuture!(self.cachelib.get_multiple_from_cachelib(keys));

        let left_to_fetch: Vec<_> = left_to_fetch
            .into_iter()
            .map(move |(key, cache_key)| {
                let memcache_key = self.get_memcache_key(&cache_key);
                (key, cache_key, memcache_key)
            })
            .collect();

        cloned!(
            self.cachelib,
            self.get_from_db,
            self.memcache,
            self.serialize,
            self.determinator,
        );

        get_multiple_from_memcache(
            left_to_fetch,
            &self.memcache,
            self.deserialize.clone(),
            self.report_mc_result.clone(),
        )
        .then(move |result: Result<_, !>| {
            let (fetched_from_memcache, left_to_fetch) = match result {
                Ok(result) => result,
                Err(never) => never,
            };
            let fetched_from_memcache =
                cachelib.fill_multiple_cachelib(determinator, fetched_from_memcache);

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

                let fetched_from_db = cachelib.fill_multiple_cachelib(
                    determinator,
                    fill_multiple_memcache(&determinator, fetched_from_db, memcache, serialize),
                );

                let mut fetched = HashMap::new();
                fetched.extend(fetched_from_cachelib);
                fetched.extend(fetched_from_memcache);
                fetched.extend(fetched_from_db);
                fetched
            })
        })
        .boxify()
    }

    pub fn fill_caches(&self, key_values: HashMap<Key, T>) {
        let keys: HashMap<Key, (T, CachelibKey, MemcacheKey)> = key_values
            .into_iter()
            .map(|(key, value)| {
                let cachelib_key = self.get_cachelib_key(&key);
                let memcache_key = self.get_memcache_key(&cachelib_key);
                (key, (value, cachelib_key, memcache_key))
            })
            .collect();

        self.cachelib.fill_multiple_cachelib(
            self.determinator,
            fill_multiple_memcache(
                &self.determinator,
                keys,
                self.memcache.clone(),
                self.serialize.clone(),
            ),
        );
    }

    fn get_cachelib_key(&self, key: &Key) -> CachelibKey {
        let cache_key = (self.get_cache_key)(self.repo_id, key);
        CachelibKey(cache_key)
    }

    fn get_memcache_key(&self, key: &CachelibKey) -> MemcacheKey {
        MemcacheKey(self.keygen.key(&key.0))
    }
}

fn get_multiple_from_memcache<Key, T>(
    keys: Vec<(Key, CachelibKey, MemcacheKey)>,
    memcache: &MemcacheHandler,
    deserialize: Arc<dyn Fn(Bytes) -> Result<T, ()> + Send + Sync + 'static>,
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
        .map(move |(key, cache_key, memcache_key)| {
            memcache
                .get(memcache_key.0.clone())
                .map_err(|()| McErrorKind::MemcacheInternal)
                .and_then(|maybe_serialized| maybe_serialized.ok_or(McErrorKind::Missing))
                .then(move |result| -> Result<_, !> { Ok((key, result, cache_key, memcache_key)) })
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
    determinator: &CachingDeterminator<T>,
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
                use CacheDisposition::*;
                let f = match determinator(&value) {
                    Cache => memcache.set(memcache_key.0, serialized).boxify(),
                    CacheWithTtl(duration) => memcache
                        .set_with_ttl(memcache_key.0, serialized, duration)
                        .boxify(),
                    Ignore => ok(()).boxify(),
                };
                ::tokio_old::spawn(f);
            }

            (key, (value, cache_key))
        })
        .collect()
}

#[cfg(test)]
mod test {
    use super::*;

    use abomonation_derive::Abomonation;
    use futures_old::future::ok;
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
        let deserialize = |_buf: Bytes| -> Result<u8, ()> { Ok(0) };

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
            determinator: cache_all_determinator::<u8>,
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

        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
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
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
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
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
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

        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
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

        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();

        let f = params.run(hashset! {});
        let _ = runtime.block_on(f).unwrap();
        assert_eq!(db_data_calls.load(Ordering::SeqCst), 0);
    }

    #[derive(Abomonation, Clone, Debug, PartialEq, Eq)]
    struct TestEntity(Vec<u8>);

    impl MemcacheEntity for TestEntity {
        fn serialize(&self) -> Bytes {
            Bytes::from(self.0.clone())
        }

        fn deserialize(bytes: Bytes) -> Result<Self, ()> {
            Ok(Self(bytes.to_vec()))
        }

        fn report_mc_result(_: &McResult<Self>) {}
    }

    struct TestStore {
        keygen: KeyGen,
        cachelib: CachelibHandler<TestEntity>,
        memcache: MemcacheHandler,
        calls: AtomicUsize,
        keys: AtomicUsize,
        data: HashMap<String, TestEntity>,
    }

    impl TestStore {
        pub fn new() -> Self {
            Self {
                keygen: KeyGen::new("", 0, 0),
                cachelib: CachelibHandler::create_mock(),
                memcache: MemcacheHandler::create_mock(),
                calls: AtomicUsize::new(0),
                keys: AtomicUsize::new(0),
                data: HashMap::new(),
            }
        }
    }

    impl EntityStore<TestEntity> for TestStore {
        fn cachelib(&self) -> &CachelibHandler<TestEntity> {
            &self.cachelib
        }

        fn keygen(&self) -> &KeyGen {
            &self.keygen
        }

        fn memcache(&self) -> &MemcacheHandler {
            &self.memcache
        }

        fn cache_determinator(&self, _: &TestEntity) -> CacheDispositionNew {
            CacheDispositionNew::Cache(CacheTtl::NoTtl)
        }

        fn spawn_memcache_writes(&self) -> bool {
            false
        }
    }

    #[async_trait]
    impl KeyedEntityStore<String, TestEntity> for TestStore {
        fn get_cache_key(&self, key: &String) -> String {
            format!("key:{}", key)
        }

        async fn get_from_db(
            &self,
            keys: HashSet<String>,
        ) -> Result<HashMap<String, TestEntity>, Error> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.keys.fetch_add(keys.len(), Ordering::Relaxed);

            Ok(keys
                .into_iter()
                .filter_map(|k| {
                    let v = self.data.get(&k).cloned();
                    v.map(|v| (k, v))
                })
                .collect())
        }
    }

    #[tokio::test]
    async fn simple_new() -> Result<(), Error> {
        let store = TestStore::new();

        let res = get_or_fill(&store, hashset! {}).await?;
        assert_eq!(res.len(), 0);
        assert_eq!(store.cachelib.gets_count(), 0);
        assert_eq!(store.memcache.gets_count(), 0);

        let res = get_or_fill(&store, hashset! {"key".into()}).await?;
        assert_eq!(res.len(), 0);
        assert_eq!(store.cachelib.gets_count(), 1);
        assert_eq!(store.memcache.gets_count(), 1);
        assert_eq!(store.keys.load(Ordering::Relaxed), 1);

        Ok(())
    }

    #[tokio::test]
    async fn fetch_from_db_cachelib_memcache_new() -> Result<(), Error> {
        let mut store = TestStore::new();

        let e = TestEntity(vec![0]);
        store.data.insert("key".into(), e.clone());

        // Fetch from db
        let res = get_or_fill(&store, hashset! {"key".into()}).await?;
        assert_eq!(res, hashmap! { "key".into() => e.clone() });
        assert_eq!(store.cachelib.gets_count(), 1);
        assert_eq!(store.memcache.gets_count(), 1);
        assert_eq!(store.keys.load(Ordering::Relaxed), 1);

        // Now fetch from cachelib
        let res = get_or_fill(&store, hashset! {"key".into()}).await?;
        assert_eq!(res, hashmap! { "key".into() => e.clone() });
        assert_eq!(store.cachelib.gets_count(), 2);
        assert_eq!(store.memcache.gets_count(), 1);
        assert_eq!(store.keys.load(Ordering::Relaxed), 1);

        // Reset cachelib, fetch from memcache
        store.cachelib = CachelibHandler::create_mock();
        let res = get_or_fill(&store, hashset! {"key".into()}).await?;
        assert_eq!(res, hashmap! { "key".into() => e.clone() });
        assert_eq!(store.cachelib.gets_count(), 1);
        assert_eq!(store.memcache.gets_count(), 2);
        assert_eq!(store.keys.load(Ordering::Relaxed), 1);

        Ok(())
    }

    #[tokio::test]
    async fn fetch_from_db_new() -> Result<(), Error> {
        let mut store = TestStore::new();

        let e0 = TestEntity(vec![0]);
        let e1 = TestEntity(vec![1]);
        let e2 = TestEntity(vec![2]);

        store.data.insert("key0".into(), e0.clone());
        store.data.insert("key1".into(), e1.clone());
        store.data.insert("key2".into(), e2.clone());

        let res = get_or_fill(
            &store,
            hashset! { "key0".into(), "key1".into(), "key2".into() },
        )
        .await?;

        assert_eq!(
            res,
            hashmap! { "key0".into() => e0, "key1".into() => e1, "key2".into() => e2 }
        );
        assert_eq!(store.cachelib.gets_count(), 3);
        assert_eq!(store.memcache.gets_count(), 3);
        assert_eq!(store.keys.load(Ordering::Relaxed), 3);

        Ok(())
    }

    #[tokio::test]
    async fn fetch_from_all_new() -> Result<(), Error> {
        let mut store = TestStore::new();

        let e0 = TestEntity(vec![0]);
        let e1 = TestEntity(vec![1]);
        let e2 = TestEntity(vec![2]);

        store.data.insert("key0".into(), e0.clone());
        store.data.insert("key1".into(), e1.clone());
        store.data.insert("key2".into(), e2.clone());

        let res = get_or_fill(&store, hashset! { "key1".into() }).await?;
        assert_eq!(res, hashmap! { "key1".into() => e1.clone() });
        assert_eq!(store.cachelib.gets_count(), 1);
        assert_eq!(store.memcache.gets_count(), 1);
        assert_eq!(store.calls.load(Ordering::Relaxed), 1);

        // Reset cachelib
        store.cachelib = CachelibHandler::create_mock();
        let res = get_or_fill(&store, hashset! { "key0".into() }).await?;
        assert_eq!(res, hashmap! { "key0".into() => e0.clone() });
        assert_eq!(store.cachelib.gets_count(), 1);
        assert_eq!(store.memcache.gets_count(), 2);
        assert_eq!(store.calls.load(Ordering::Relaxed), 2);

        let res = get_or_fill(
            &store,
            hashset! { "key0".into(), "key1".into(), "key2".into() },
        )
        .await?;

        assert_eq!(
            res,
            hashmap! { "key0".into() => e0.clone(), "key1".into() => e1.clone(), "key2".into() => e2.clone() }
        );
        assert_eq!(store.cachelib.gets_count(), 1 + 3); // 3 new fetches from cachelib, 2 misses
        assert_eq!(store.memcache.gets_count(), 2 + 2); // 2 new fetches from memcache, 1 miss
        assert_eq!(store.calls.load(Ordering::Relaxed), 2 + 1); // 1 fetch from db

        // Only from cachelib
        let res = get_or_fill(
            &store,
            hashset! { "key0".into(), "key1".into(), "key2".into() },
        )
        .await?;

        assert_eq!(
            res,
            hashmap! { "key0".into() => e0.clone(), "key1".into() => e1.clone(), "key2".into() => e2.clone() }
        );
        assert_eq!(store.cachelib.gets_count(), 7);
        assert_eq!(store.memcache.gets_count(), 4);
        assert_eq!(store.calls.load(Ordering::Relaxed), 3);

        // // Reset cachelib, only from memcache
        store.cachelib = CachelibHandler::create_mock();
        let res = get_or_fill(
            &store,
            hashset! { "key0".into(), "key1".into(), "key2".into() },
        )
        .await?;

        assert_eq!(
            res,
            hashmap! { "key0".into() => e0.clone(), "key1".into() => e1.clone(), "key2".into() => e2.clone() }
        );
        assert_eq!(store.cachelib.gets_count(), 3); // 3 misses
        assert_eq!(store.memcache.gets_count(), 4 + 3); // 3 hits
        assert_eq!(store.calls.load(Ordering::Relaxed), 3);

        Ok(())
    }

    #[tokio::test]
    async fn get_from_db_elision_new() -> Result<(), Error> {
        let store = TestStore::new();

        get_or_fill(&store, hashset! {}).await?;
        assert_eq!(store.calls.load(Ordering::Relaxed), 0);

        Ok(())
    }
}
