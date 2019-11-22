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

use anyhow::Result;

use configparser::{config::ConfigSet, hg::ConfigSetHgExt};
use types::Key;

use crate::{
    datastore::{DataStore, Delta, Metadata, MutableDeltaStore, RemoteDataStore},
    indexedlogdatastore::IndexedLogDataStore,
    localstore::LocalStore,
    packstore::{CorruptionPolicy, MutableDataPackStore},
    remotestore::RemoteStore,
    uniondatastore::UnionDataStore,
    util::{get_cache_indexedlogdatastore_path, get_cache_packs_path, get_local_packs_path},
};

struct ContentStoreInner {
    datastore: UnionDataStore<Box<dyn DataStore>>,
    local_mutabledatastore: Box<dyn MutableDeltaStore>,
    shared_mutabledatastore: Box<dyn MutableDeltaStore>,
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
        ContentStoreBuilder::new(&local_path, config).build()
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
    fn prefetch(&self, keys: Vec<Key>) -> Result<()> {
        if let Some(remote_store) = self.inner.remote_store.as_ref() {
            let missing = self.get_missing(&keys)?;
            if missing == vec![] {
                Ok(())
            } else {
                remote_store.prefetch(missing)
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
impl MutableDeltaStore for ContentStore {
    /// Add the data to the local store.
    fn add(&self, delta: &Delta, metadata: &Metadata) -> Result<()> {
        self.inner.local_mutabledatastore.add(delta, metadata)
    }

    /// Commit the data written to the local store.
    fn flush(&self) -> Result<Option<PathBuf>> {
        self.inner.local_mutabledatastore.flush()
    }
}

/// Builder for `ContentStore`. An `impl AsRef<Path>` represents the path to the store and a
/// `ConfigSet` of the Mercurial configuration are required to build a `ContentStore`. Users can
/// use this builder to add optional `RemoteStore` to enable remote data fetching， and a `Path`
/// suffix to specify other type of stores.
pub struct ContentStoreBuilder<'a> {
    local_path: PathBuf,
    config: &'a ConfigSet,
    remotestore: Option<Box<dyn RemoteStore>>,
    suffix: Option<&'a Path>,
}

impl<'a> ContentStoreBuilder<'a> {
    pub fn new(local_path: impl AsRef<Path>, config: &'a ConfigSet) -> Self {
        Self {
            local_path: local_path.as_ref().to_path_buf(),
            config,
            remotestore: None,
            suffix: None,
        }
    }

    pub fn remotestore(mut self, remotestore: Box<dyn RemoteStore>) -> Self {
        self.remotestore = Some(remotestore);
        self
    }

    pub fn suffix(mut self, suffix: &'a Path) -> Self {
        self.suffix = Some(suffix);
        self
    }

    pub fn build(self) -> Result<ContentStore> {
        let cache_packs_path = get_cache_packs_path(self.config, self.suffix)?;
        let local_pack_store = Box::new(MutableDataPackStore::new(
            get_local_packs_path(self.local_path, self.suffix)?,
            CorruptionPolicy::IGNORE,
        )?);
        let shared_pack_store = Box::new(MutableDataPackStore::new(
            &cache_packs_path,
            CorruptionPolicy::REMOVE,
        )?);
        let mut datastore: UnionDataStore<Box<dyn DataStore>> = UnionDataStore::new();

        if self
            .config
            .get_or_default::<bool>("remotefilelog", "indexedlogdatastore")?
        {
            let shared_indexedlogdatastore = Box::new(IndexedLogDataStore::new(
                get_cache_indexedlogdatastore_path(self.config)?,
            )?);
            datastore.add(shared_indexedlogdatastore);
        }

        datastore.add(shared_pack_store.clone());
        datastore.add(local_pack_store.clone());

        let remote_store: Option<Arc<dyn RemoteDataStore>> =
            if let Some(remotestore) = self.remotestore {
                let store = remotestore.datastore(shared_pack_store.clone());
                datastore.add(Box::new(store.clone()));
                Some(store)
            } else {
                None
            };

        let local_mutabledatastore: Box<dyn MutableDeltaStore> = local_pack_store;
        let shared_mutabledatastore: Box<dyn MutableDeltaStore> = shared_pack_store;

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

    use crate::testutil::FakeRemoteStore;

    fn make_config(dir: impl AsRef<Path>) -> ConfigSet {
        let mut config = ConfigSet::new();

        config.set(
            "remotefilelog",
            "reponame",
            Some(b"test"),
            &Default::default(),
        );
        config.set(
            "remotefilelog",
            "cachepath",
            Some(dir.as_ref().to_str().unwrap().as_bytes()),
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

        let store = ContentStoreBuilder::new(&localdir, &config)
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

        let store = ContentStoreBuilder::new(&localdir, &config)
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

        let store = ContentStoreBuilder::new(&localdir, &config)
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

        let store = ContentStoreBuilder::new(&localdir, &config)
            .remotestore(Box::new(remotestore))
            .build()?;
        store.get(&k)?;
        store.inner.shared_mutabledatastore.get(&k)?;
        assert!(store.inner.local_mutabledatastore.get(&k)?.is_none());
        Ok(())
    }
}
