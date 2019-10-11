// Copyright Facebook, Inc. 2019

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use failure::{format_err, Fallible};

use configparser::config::ConfigSet;
use edenapi::EdenApi;
use types::Key;
use util::path::create_dir;

use crate::{
    datastore::{DataStore, Delta, Metadata, MutableDeltaStore, RemoteDataStore},
    edenapi::EdenApiRemoteStore,
    indexedlogdatastore::IndexedLogDataStore,
    localstore::LocalStore,
    packstore::{CorruptionPolicy, MutableDataPackStore},
    uniondatastore::UnionDataStore,
};

struct ContentStoreInner {
    datastore: UnionDataStore<Box<dyn DataStore>>,
    local_mutabledatastore: Box<dyn MutableDeltaStore>,
    shared_mutabledatastore: Box<dyn MutableDeltaStore>,
    remote_store: Option<Box<dyn RemoteDataStore>>,
}

/// A `ContentStore` aggregate all the local and remote stores and expose them as one. Both local and
/// remote stores can be queried and accessed via the `DataStore` trait. The local store can also
/// be written to via the `MutableDeltaStore` trait, this is intended to be used to store local
/// commit data.
#[derive(Clone)]
pub struct ContentStore {
    inner: Arc<ContentStoreInner>,
}

fn get_repo_name(config: &ConfigSet) -> Fallible<String> {
    let name = config
        .get("remotefilelog", "reponame")
        .ok_or_else(|| format_err!("remotefilelog.reponame is not set"))?;
    Ok(String::from_utf8(name.to_vec())?)
}

fn get_cache_path(config: &ConfigSet) -> Fallible<PathBuf> {
    let reponame = get_repo_name(config)?;
    let config_path = config
        .get("remotefilelog", "cachepath")
        .ok_or_else(|| format_err!("remotefilelog.cachepath is not set"))?;
    let mut path = PathBuf::new();
    path.push(String::from_utf8(config_path.to_vec())?);
    path.push(reponame);
    create_dir(&path)?;
    Ok(path)
}

fn get_cache_packs_path(config: &ConfigSet) -> Fallible<PathBuf> {
    let mut path = get_cache_path(config)?;
    path.push("packs");
    create_dir(&path)?;
    Ok(path)
}

fn get_cache_indexedlogdatastore_path(config: &ConfigSet) -> Fallible<PathBuf> {
    let mut path = get_cache_path(config)?;
    path.push("indexedlogdatastore");
    create_dir(&path)?;
    Ok(path)
}

fn get_local_packs_path(path: impl AsRef<Path>) -> Fallible<PathBuf> {
    let mut path = path.as_ref().to_owned();
    path.push("packs");
    create_dir(&path)?;
    Ok(path)
}

impl ContentStore {
    pub fn new(
        local_path: impl AsRef<Path>,
        config: &ConfigSet,
        edenapi: Option<Box<dyn EdenApi>>,
    ) -> Fallible<Self> {
        let cache_packs_path = get_cache_packs_path(config)?;
        let local_pack_store = Box::new(MutableDataPackStore::new(
            get_local_packs_path(local_path)?,
            CorruptionPolicy::IGNORE,
        )?);
        let shared_pack_store = Box::new(MutableDataPackStore::new(
            &cache_packs_path,
            CorruptionPolicy::REMOVE,
        )?);
        let shared_indexedlogdatastore = Box::new(IndexedLogDataStore::new(
            get_cache_indexedlogdatastore_path(config)?,
        )?);

        let mut datastore: UnionDataStore<Box<dyn DataStore>> = UnionDataStore::new();

        datastore.add(shared_indexedlogdatastore);
        datastore.add(shared_pack_store.clone());
        datastore.add(local_pack_store.clone());

        let remote_store: Option<Box<dyn RemoteDataStore>> = if let Some(edenapi) = edenapi {
            let store = Box::new(EdenApiRemoteStore::new(edenapi, shared_pack_store.clone()));
            datastore.add(store.clone());
            Some(store)
        } else {
            None
        };

        let local_mutabledatastore: Box<dyn MutableDeltaStore> = local_pack_store;
        let shared_mutabledatastore: Box<dyn MutableDeltaStore> = shared_pack_store;

        Ok(Self {
            inner: Arc::new(ContentStoreInner {
                datastore,
                local_mutabledatastore,
                shared_mutabledatastore,
                remote_store,
            }),
        })
    }
}

impl DataStore for ContentStore {
    fn get(&self, key: &Key) -> Fallible<Option<Vec<u8>>> {
        self.inner.datastore.get(key)
    }

    fn get_delta(&self, key: &Key) -> Fallible<Option<Delta>> {
        self.inner.datastore.get_delta(key)
    }

    fn get_delta_chain(&self, key: &Key) -> Fallible<Option<Vec<Delta>>> {
        self.inner.datastore.get_delta_chain(key)
    }

    fn get_meta(&self, key: &Key) -> Fallible<Option<Metadata>> {
        self.inner.datastore.get_meta(key)
    }
}

impl RemoteDataStore for ContentStore {
    fn prefetch(&self, keys: Vec<Key>) -> Fallible<()> {
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
    fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>> {
        self.inner.datastore.get_missing(keys)
    }
}

impl Drop for ContentStore {
    /// The shared store is a cache, so let's flush all pending data when the `ContentStore` goes
    /// out of scope.
    fn drop(&mut self) {
        let _ = self.inner.shared_mutabledatastore.flush();
    }
}

/// MutableDeltaStore is only implemented for the local store and not for the remote ones. The
/// remote stores will be automatically written to while calling the various `DataStore` methods.
impl MutableDeltaStore for ContentStore {
    /// Add the data to the local store.
    fn add(&self, delta: &Delta, metadata: &Metadata) -> Fallible<()> {
        self.inner.local_mutabledatastore.add(delta, metadata)
    }

    /// Commit the data written to the local store.
    fn flush(&self) -> Fallible<Option<PathBuf>> {
        self.inner.local_mutabledatastore.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    use bytes::Bytes;
    use tempfile::TempDir;

    use types::testutil::*;

    use crate::testutil::fake_edenapi;

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
    fn test_new() -> Fallible<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let _store = ContentStore::new(&localdir, &config, None)?;
        Ok(())
    }

    #[test]
    fn test_add_get() -> Fallible<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let store = ContentStore::new(&localdir, &config, None)?;

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
    fn test_add_dropped() -> Fallible<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let store = ContentStore::new(&localdir, &config, None)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: Some(key("a", "1")),
            key: k1.clone(),
        };
        store.add(&delta, &Default::default())?;
        drop(store);

        let store = ContentStore::new(&localdir, &config, None)?;
        assert!(store.get_delta(&k1)?.is_none());
        Ok(())
    }

    #[test]
    fn test_add_flush_get() -> Fallible<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let store = ContentStore::new(&localdir, &config, None)?;

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
    fn test_add_flush_drop_get() -> Fallible<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let store = ContentStore::new(&localdir, &config, None)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: Some(key("a", "1")),
            key: k1.clone(),
        };
        store.add(&delta, &Default::default())?;
        store.flush()?;
        drop(store);

        let store = ContentStore::new(&localdir, &config, None)?;
        assert_eq!(store.get_delta(&k1)?, Some(delta));
        Ok(())
    }

    #[test]
    fn test_remote_store() -> Fallible<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let k = key("a", "1");
        let data = Bytes::from(&[1, 2, 3, 4][..]);

        let mut map = HashMap::new();
        map.insert(k.clone(), data.clone());

        let edenapi = fake_edenapi(map);

        let store = ContentStore::new(&localdir, &config, Some(edenapi))?;
        let data_get = store.get(&k)?;

        assert_eq!(data_get.unwrap(), data);
        Ok(())
    }

    #[test]
    fn test_remote_store_cached() -> Fallible<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let k = key("a", "1");
        let data = Bytes::from(&[1, 2, 3, 4][..]);

        let mut map = HashMap::new();
        map.insert(k.clone(), data.clone());

        let edenapi = fake_edenapi(map);

        let store = ContentStore::new(&localdir, &config, Some(edenapi))?;
        store.get(&k)?;
        drop(store);

        let store = ContentStore::new(&localdir, &config, None)?;
        let data_get = store.get(&k)?;

        assert_eq!(data_get.unwrap(), data);

        Ok(())
    }

    #[test]
    fn test_not_in_remote_store() -> Fallible<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let map = HashMap::new();
        let edenapi = fake_edenapi(map);

        let store = ContentStore::new(&localdir, &config, Some(edenapi))?;

        let k = key("a", "1");
        assert_eq!(store.get(&k)?, None);
        Ok(())
    }

    #[test]
    fn test_fetch_location() -> Fallible<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let k = key("a", "1");
        let data = Bytes::from(&[1, 2, 3, 4][..]);

        let mut map = HashMap::new();
        map.insert(k.clone(), data.clone());

        let edenapi = fake_edenapi(map);

        let store = ContentStore::new(&localdir, &config, Some(edenapi))?;
        store.get(&k)?;
        store.inner.shared_mutabledatastore.get(&k)?;
        assert!(store.inner.local_mutabledatastore.get(&k)?.is_none());
        Ok(())
    }
}
