/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{format_err, Result};
use bytes::Bytes;

use configparser::{config::ConfigSet, hg::ConfigSetHgExt};
use types::Key;

use crate::{
    datastore::{strip_metadata, DataStore, Delta, Metadata, MutableDeltaStore, RemoteDataStore},
    indexedlogdatastore::IndexedLogDataStore,
    lfs::LfsStore,
    localstore::LocalStore,
    memcache::MemcacheStore,
    multiplexstore::MultiplexDeltaStore,
    packstore::{CorruptionPolicy, MutableDataPackStore},
    remotestore::RemoteStore,
    uniondatastore::UnionDataStore,
    util::{
        get_cache_packs_path, get_cache_path, get_indexedlogdatastore_path, get_local_path,
        get_packs_path,
    },
};

struct ContentStoreInner {
    datastore: UnionDataStore<Arc<dyn DataStore>>,
    local_mutabledatastore: Option<Arc<dyn MutableDeltaStore>>,
    shared_mutabledatastore: Arc<dyn MutableDeltaStore>,
    remote_store: Option<Arc<dyn RemoteDataStore>>,
}

/// A `ContentStore` aggregate all the local and remote stores and expose them as one. Both local and
/// remote stores can be queried and accessed via the `DataStore` trait. The local store can also
/// be written to via the `MutableDeltaStore` trait, this is intended to be used to store local
/// commit data.
#[derive(Clone)]
pub struct ContentStore {
    inner: Arc<ContentStoreInner>,
}

impl ContentStore {
    pub fn new(local_path: impl AsRef<Path>, config: &ConfigSet) -> Result<Self> {
        ContentStoreBuilder::new(config)
            .local_path(&local_path)
            .build()
    }

    /// Some blobs may contain copy-from metadata, let's strip it. For more details about the
    /// copy-from metadata, see `datastore::strip_metadata`.
    ///
    /// XXX: This should only be used on `ContentStore` that are storing actual
    /// file content, tree stores should use the `get` method instead.
    pub fn get_file_content(&self, key: &Key) -> Result<Option<Bytes>> {
        if let Some(vec) = self.get(key)? {
            let bytes = vec.into();
            let (bytes, _) = strip_metadata(&bytes)?;
            Ok(Some(bytes))
        } else {
            Ok(None)
        }
    }
}

impl DataStore for ContentStore {
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>> {
        self.inner.datastore.get(key)
    }

    fn get_delta(&self, key: &Key) -> Result<Option<Delta>> {
        self.inner.datastore.get_delta(key)
    }

    fn get_delta_chain(&self, key: &Key) -> Result<Option<Vec<Delta>>> {
        self.inner.datastore.get_delta_chain(key)
    }

    fn get_meta(&self, key: &Key) -> Result<Option<Metadata>> {
        self.inner.datastore.get_meta(key)
    }
}

impl RemoteDataStore for ContentStore {
    fn prefetch(&self, keys: &[Key]) -> Result<()> {
        if let Some(remote_store) = self.inner.remote_store.as_ref() {
            let missing = self.get_missing(keys)?;
            if missing == vec![] {
                Ok(())
            } else {
                remote_store.prefetch(&missing)
            }
        } else {
            // There is no remote store, let's pretend everything is fine.
            Ok(())
        }
    }
}

impl LocalStore for ContentStore {
    fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>> {
        self.inner.datastore.get_missing(keys)
    }
}

impl Drop for ContentStoreInner {
    /// The shared store is a cache, so let's flush all pending data when the `ContentStore` goes
    /// out of scope.
    fn drop(&mut self) {
        let _ = self.shared_mutabledatastore.flush();
    }
}

/// MutableDeltaStore is only implemented for the local store and not for the remote ones. The
/// remote stores will be automatically written to while calling the various `DataStore` methods.
///
/// These methods can only be used when the ContentStore was created with a local store.
impl MutableDeltaStore for ContentStore {
    /// Add the data to the local store.
    fn add(&self, delta: &Delta, metadata: &Metadata) -> Result<()> {
        self.inner
            .local_mutabledatastore
            .as_ref()
            .ok_or_else(|| format_err!("writing to a non-local ContentStore is not allowed"))?
            .add(delta, metadata)
    }

    /// Commit the data written to the local store.
    fn flush(&self) -> Result<Option<PathBuf>> {
        self.inner
            .local_mutabledatastore
            .as_ref()
            .ok_or_else(|| format_err!("flushing a non-local ContentStore is not allowed"))?
            .flush()
    }
}

/// Builder for `ContentStore`. An `impl AsRef<Path>` represents the path to the store and a
/// `ConfigSet` of the Mercurial configuration are required to build a `ContentStore`. Users can
/// use this builder to add optional `RemoteStore` to enable remote data fetching， and a `Path`
/// suffix to specify other type of stores.
pub struct ContentStoreBuilder<'a> {
    local_path: Option<PathBuf>,
    no_local_store: bool,
    config: &'a ConfigSet,
    remotestore: Option<Box<dyn RemoteStore>>,
    suffix: Option<PathBuf>,
    memcachestore: Option<MemcacheStore>,
}

impl<'a> ContentStoreBuilder<'a> {
    pub fn new(config: &'a ConfigSet) -> Self {
        Self {
            local_path: None,
            no_local_store: false,
            config,
            remotestore: None,
            memcachestore: None,
            suffix: None,
        }
    }

    /// Path to the local store.
    pub fn local_path(mut self, local_path: impl AsRef<Path>) -> Self {
        self.local_path = Some(local_path.as_ref().to_path_buf());
        self
    }

    /// Allows a ContentStore to be created without a local store.
    ///
    /// This should be used in very specific cases that do not want a local store. Unless you know
    /// exactly that this is what you want, do not use.
    pub fn no_local_store(mut self) -> Self {
        self.no_local_store = true;
        self
    }

    pub fn remotestore(mut self, remotestore: Box<dyn RemoteStore>) -> Self {
        self.remotestore = Some(remotestore);
        self
    }

    pub fn memcachestore(mut self, memcachestore: MemcacheStore) -> Self {
        self.memcachestore = Some(memcachestore);
        self
    }

    pub fn suffix(mut self, suffix: impl AsRef<Path>) -> Self {
        self.suffix = Some(suffix.as_ref().to_path_buf());
        self
    }

    pub fn build(self) -> Result<ContentStore> {
        let local_path = get_local_path(&self.local_path, &self.suffix)?;
        let cache_path = get_cache_path(self.config, &self.suffix)?;

        let cache_packs_path = get_cache_packs_path(self.config, &self.suffix)?;
        let shared_pack_store = Arc::new(MutableDataPackStore::new(
            &cache_packs_path,
            CorruptionPolicy::REMOVE,
        )?);
        let mut datastore: UnionDataStore<Arc<dyn DataStore>> = UnionDataStore::new();

        if self
            .config
            .get_or_default::<bool>("remotefilelog", "indexedlogdatastore")?
        {
            let shared_indexedlogdatastore = Arc::new(IndexedLogDataStore::new(
                get_indexedlogdatastore_path(&cache_path)?,
            )?);
            datastore.add(shared_indexedlogdatastore);
        }

        // The shared stores should precede the local one since we expect both the number of blobs,
        // and the number of requests satisfied by the shared cache to be significantly higher than
        // ones in the local store.
        datastore.add(shared_pack_store.clone());
        datastore.add(Arc::new(LfsStore::shared(&cache_path)?));

        let local_mutabledatastore: Option<Arc<dyn MutableDeltaStore>> =
            if let Some(local_path) = self.local_path {
                let local_pack_store = Arc::new(MutableDataPackStore::new(
                    get_packs_path(&local_path, &self.suffix)?,
                    CorruptionPolicy::IGNORE,
                )?);
                datastore.add(local_pack_store.clone());

                Some(local_pack_store)
            } else {
                if !self.no_local_store {
                    return Err(format_err!(
                        "a ContentStore cannot be built without a local store"
                    ));
                }
                None
            };

        if let Some(local_path) = local_path {
            datastore.add(Arc::new(LfsStore::local(&local_path)?));
        }

        let remote_store: Option<Arc<dyn RemoteDataStore>> =
            if let Some(remotestore) = self.remotestore {
                let (cache, shared_store) = if let Some(memcachestore) = self.memcachestore {
                    // Combine the memcache store with the other stores. The intent is that all
                    // remote requests will first go to the memcache store, and only reach the
                    // slower remote store after that.
                    //
                    // If data isn't found in the memcache store, once fetched from the remote
                    // store it will be written to the local cache, and will populate the memcache
                    // store, so other clients and future requests won't need to go to a network
                    // store.
                    let memcachedatastore = memcachestore.datastore(shared_pack_store.clone());

                    let mut multiplexstore: MultiplexDeltaStore<Arc<dyn MutableDeltaStore>> =
                        MultiplexDeltaStore::new();
                    multiplexstore.add_store(Arc::new(memcachestore));
                    multiplexstore.add_store(shared_pack_store.clone());

                    (
                        Some(memcachedatastore),
                        Arc::new(multiplexstore) as Arc<dyn MutableDeltaStore>,
                    )
                } else {
                    (
                        None,
                        shared_pack_store.clone() as Arc<dyn MutableDeltaStore>,
                    )
                };

                let store = remotestore.datastore(shared_store);

                let remotestores = if let Some(cache) = cache {
                    let mut remotestores = UnionDataStore::new();
                    remotestores.add(cache.clone());
                    remotestores.add(store.clone());
                    Arc::new(remotestores)
                } else {
                    store
                };

                datastore.add(Arc::new(remotestores.clone()));
                Some(remotestores)
            } else {
                None
            };

        let shared_mutabledatastore: Arc<dyn MutableDeltaStore> = shared_pack_store;

        Ok(ContentStore {
            inner: Arc::new(ContentStoreInner {
                datastore,
                local_mutabledatastore,
                shared_mutabledatastore,
                remote_store,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    use bytes::Bytes;
    use tempfile::TempDir;

    use types::testutil::*;
    use util::path::create_dir;

    use crate::testutil::FakeRemoteStore;

    fn make_config(dir: impl AsRef<Path>) -> ConfigSet {
        let mut config = ConfigSet::new();

        config.set(
            "remotefilelog",
            "reponame",
            Some("test"),
            &Default::default(),
        );
        config.set(
            "remotefilelog",
            "cachepath",
            Some(dir.as_ref().to_str().unwrap()),
            &Default::default(),
        );

        config.set(
            "remotefilelog",
            "cachekey",
            Some("cca:hg:rust_unittest"),
            &Default::default(),
        );

        config
    }

    #[test]
    fn test_new() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let _store = ContentStore::new(&localdir, &config)?;
        Ok(())
    }

    #[test]
    fn test_add_get() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let store = ContentStore::new(&localdir, &config)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: Some(key("a", "1")),
            key: k1.clone(),
        };
        store.add(&delta, &Default::default())?;
        assert_eq!(store.get_delta(&k1)?, Some(delta));
        Ok(())
    }

    #[test]
    fn test_add_dropped() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let store = ContentStore::new(&localdir, &config)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: Some(key("a", "1")),
            key: k1.clone(),
        };
        store.add(&delta, &Default::default())?;
        drop(store);

        let store = ContentStore::new(&localdir, &config)?;
        assert!(store.get_delta(&k1)?.is_none());
        Ok(())
    }

    #[test]
    fn test_add_flush_get() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let store = ContentStore::new(&localdir, &config)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: Some(key("a", "1")),
            key: k1.clone(),
        };
        store.add(&delta, &Default::default())?;
        store.flush()?;
        assert_eq!(store.get_delta(&k1)?, Some(delta));
        Ok(())
    }

    #[test]
    fn test_add_flush_drop_get() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let store = ContentStore::new(&localdir, &config)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: Some(key("a", "1")),
            key: k1.clone(),
        };
        store.add(&delta, &Default::default())?;
        store.flush()?;
        drop(store);

        let store = ContentStore::new(&localdir, &config)?;
        assert_eq!(store.get_delta(&k1)?, Some(delta));
        Ok(())
    }

    #[test]
    fn test_remote_store() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let k = key("a", "1");
        let data = Bytes::from(&[1, 2, 3, 4][..]);

        let mut map = HashMap::new();
        map.insert(k.clone(), data.clone());
        let mut remotestore = FakeRemoteStore::new();
        remotestore.data(map);

        let store = ContentStoreBuilder::new(&config)
            .local_path(&localdir)
            .remotestore(Box::new(remotestore))
            .build()?;
        let data_get = store.get(&k)?;

        assert_eq!(data_get.unwrap(), data);
        Ok(())
    }

    #[test]
    fn test_remote_store_cached() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let k = key("a", "1");
        let data = Bytes::from(&[1, 2, 3, 4][..]);

        let mut map = HashMap::new();
        map.insert(k.clone(), data.clone());

        let mut remotestore = FakeRemoteStore::new();
        remotestore.data(map);

        let store = ContentStoreBuilder::new(&config)
            .local_path(&localdir)
            .remotestore(Box::new(remotestore))
            .build()?;
        store.get(&k)?;
        drop(store);

        let store = ContentStore::new(&localdir, &config)?;
        let data_get = store.get(&k)?;

        assert_eq!(data_get.unwrap(), data);

        Ok(())
    }

    #[test]
    fn test_not_in_remote_store() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let map = HashMap::new();
        let mut remotestore = FakeRemoteStore::new();
        remotestore.data(map);

        let store = ContentStoreBuilder::new(&config)
            .local_path(&localdir)
            .remotestore(Box::new(remotestore))
            .build()?;

        let k = key("a", "1");
        assert_eq!(store.get(&k)?, None);
        Ok(())
    }

    #[test]
    fn test_fetch_location() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let k = key("a", "1");
        let data = Bytes::from(&[1, 2, 3, 4][..]);

        let mut map = HashMap::new();
        map.insert(k.clone(), data.clone());

        let mut remotestore = FakeRemoteStore::new();
        remotestore.data(map);

        let store = ContentStoreBuilder::new(&config)
            .local_path(&localdir)
            .remotestore(Box::new(remotestore))
            .build()?;
        store.get(&k)?;
        store.inner.shared_mutabledatastore.get(&k)?;
        assert!(store
            .inner
            .local_mutabledatastore
            .as_ref()
            .unwrap()
            .get(&k)?
            .is_none());
        Ok(())
    }

    #[test]
    fn test_add_shared_only_store() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let store = ContentStore::new(&localdir, &config)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: Some(key("a", "1")),
            key: k1.clone(),
        };
        store.add(&delta, &Default::default())?;
        store.flush()?;

        let store = ContentStoreBuilder::new(&config).no_local_store().build()?;
        assert_eq!(store.get(&k1)?, None);
        Ok(())
    }

    #[test]
    fn test_no_local_store() -> Result<()> {
        let cachedir = TempDir::new()?;
        let config = make_config(&cachedir);
        assert!(ContentStoreBuilder::new(&config).build().is_err());
        Ok(())
    }

    #[test]
    fn test_lfs_local() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let lfs_store = LfsStore::local(&localdir)?;
        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k1.clone(),
        };
        lfs_store.add(&delta, &Default::default())?;
        lfs_store.flush()?;

        let store = ContentStore::new(&localdir, &config)?;
        assert_eq!(store.get(&k1)?, Some(delta.data.as_ref().to_vec()));
        Ok(())
    }

    #[test]
    fn test_lfs_shared() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let mut lfs_cache_dir = cachedir.path().to_path_buf();
        lfs_cache_dir.push("test");
        create_dir(&lfs_cache_dir)?;
        let lfs_store = LfsStore::shared(&lfs_cache_dir)?;
        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k1.clone(),
        };
        lfs_store.add(&delta, &Default::default())?;
        lfs_store.flush()?;

        let store = ContentStore::new(&localdir, &config)?;
        assert_eq!(store.get(&k1)?, Some(delta.data.as_ref().to_vec()));
        Ok(())
    }

    #[cfg(fbcode_build)]
    mod fbcode_tests {
        use super::*;

        use memcache::MockMemcache;

        use lazy_static::lazy_static;

        lazy_static! {
            static ref MOCK: Arc<MockMemcache> = Arc::new(MockMemcache::new());
        }

        #[fbinit::test]
        fn test_memcache_get() -> Result<()> {
            let _mock = Arc::clone(&*MOCK);

            let cachedir = TempDir::new()?;
            let localdir = TempDir::new()?;
            let config = make_config(&cachedir);

            let k = key("a", "1");
            let data = Bytes::from(&[1, 2, 3, 4][..]);

            let mut map = HashMap::new();
            map.insert(k.clone(), data.clone());
            let mut remotestore = FakeRemoteStore::new();
            remotestore.data(map);

            let memcache = MemcacheStore::new(&config)?;
            let store = ContentStoreBuilder::new(&config)
                .local_path(&localdir)
                .remotestore(Box::new(remotestore))
                .memcachestore(memcache.clone())
                .build()?;
            let data_get = store.get(&k)?;
            assert_eq!(data_get.unwrap(), data);

            loop {
                let memcache_data = memcache.get(&k)?;
                if let Some(memcache_data) = memcache_data {
                    assert_eq!(memcache_data, data);
                    break;
                }
            }
            Ok(())
        }

        #[fbinit::test]
        fn test_memcache_get_large() -> Result<()> {
            let _mock = Arc::clone(&*MOCK);

            let cachedir = TempDir::new()?;
            let localdir = TempDir::new()?;
            let config = make_config(&cachedir);

            let k = key("a", "1");

            let data = (0..10 * 1024 * 1024)
                .map(|_| rand::random::<u8>())
                .collect::<Bytes>();
            assert_eq!(data.len(), 10 * 1024 * 1024);

            let mut map = HashMap::new();
            map.insert(k.clone(), data.clone());
            let mut remotestore = FakeRemoteStore::new();
            remotestore.data(map);

            let memcache = MemcacheStore::new(&config)?;
            let store = ContentStoreBuilder::new(&config)
                .local_path(&localdir)
                .remotestore(Box::new(remotestore))
                .memcachestore(memcache.clone())
                .build()?;
            let data_get = store.get(&k)?;
            assert_eq!(data_get.unwrap(), data);

            let memcache_data = memcache.get(&k)?;
            assert_eq!(memcache_data, None);
            Ok(())
        }

        #[fbinit::test]
        fn test_memcache_no_wait() -> Result<()> {
            let _mock = Arc::clone(&*MOCK);

            let cachedir = TempDir::new()?;
            let localdir = TempDir::new()?;
            let mut config = make_config(&cachedir);
            config.set(
                "remotefilelog",
                "waitformemcache",
                Some("false"),
                &Default::default(),
            );

            let k = key("a", "1");
            let data = Bytes::from(&[1, 2, 3, 4][..]);

            let mut map = HashMap::new();
            map.insert(k.clone(), data.clone());
            let mut remotestore = FakeRemoteStore::new();
            remotestore.data(map);

            let memcache = MemcacheStore::new(&config)?;
            let store = ContentStoreBuilder::new(&config)
                .local_path(&localdir)
                .remotestore(Box::new(remotestore))
                .memcachestore(memcache.clone())
                .build()?;
            let data_get = store.get(&k)?;
            assert_eq!(data_get.unwrap(), data);

            // Ideally, we should check that we didn't wait for memcache, but that's timing
            // related and thus a bit hard to test.
            Ok(())
        }
    }
}
