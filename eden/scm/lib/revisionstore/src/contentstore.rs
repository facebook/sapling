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
    local_mutabledatastore: Option<Box<dyn MutableDeltaStore>>,
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
        ContentStoreBuilder::new(config)
            .local_path(&local_path)
            .build()
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
    suffix: Option<&'a Path>,
}

impl<'a> ContentStoreBuilder<'a> {
    pub fn new(config: &'a ConfigSet) -> Self {
        Self {
            local_path: None,
            no_local_store: false,
            config,
            remotestore: None,
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

    pub fn suffix(mut self, suffix: &'a Path) -> Self {
        self.suffix = Some(suffix);
        self
    }

    pub fn build(self) -> Result<ContentStore> {
        let cache_packs_path = get_cache_packs_path(self.config, self.suffix)?;
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

        // The shared stores should precede the local one since we expect both the number of blobs,
        // and the number of requests satisfied by the shared cache to be significantly higher than
        // ones in the local store.
        datastore.add(shared_pack_store.clone());

        let local_mutabledatastore: Option<Box<dyn MutableDeltaStore>> =
            if let Some(local_path) = self.local_path {
                let local_pack_store = Box::new(MutableDataPackStore::new(
                    get_local_packs_path(local_path, self.suffix)?,
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

        let remote_store: Option<Arc<dyn RemoteDataStore>> =
            if let Some(remotestore) = self.remotestore {
                let store = remotestore.datastore(shared_pack_store.clone());
                datastore.add(Box::new(store.clone()));
                Some(store)
            } else {
                None
            };

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
            Some("test"),
            &Default::default(),
        );
        config.set(
            "remotefilelog",
            "cachepath",
            Some(dir.as_ref().to_str().unwrap()),
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
}
