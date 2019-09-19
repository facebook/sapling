// Copyright Facebook, Inc. 2019

use std::path::{Path, PathBuf};

use failure::{format_err, Fallible};

use configparser::config::ConfigSet;
use types::Key;
use util::path::create_dir;

use crate::{
    datastore::{DataStore, Delta, Metadata, MutableDeltaStore},
    indexedlogdatastore::IndexedLogDataStore,
    localstore::LocalStore,
    packstore::{CorruptionPolicy, MutableDataPackStore},
    uniondatastore::UnionDataStore,
};

/// A `ContentStore` aggregate all the local and remote stores and expose them as one. Both local and
/// remote stores can be queried and accessed via the `DataStore` trait. The local store can also
/// be written to via the `MutableDeltaStore` trait, this is intended to be used to store local
/// commit data.
pub struct ContentStore {
    datastore: UnionDataStore<Box<dyn DataStore>>,
    local_mutabledatastore: Box<dyn MutableDeltaStore>,
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
    pub fn new(local_path: impl AsRef<Path>, config: &ConfigSet) -> Fallible<Self> {
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
        datastore.add(shared_pack_store);
        datastore.add(local_pack_store.clone());

        let local_mutabledatastore: Box<dyn MutableDeltaStore> = local_pack_store;

        Ok(Self {
            datastore,
            local_mutabledatastore,
        })
    }
}

impl DataStore for ContentStore {
    fn get(&self, key: &Key) -> Fallible<Vec<u8>> {
        self.datastore.get(key)
    }

    fn get_delta(&self, key: &Key) -> Fallible<Delta> {
        self.datastore.get_delta(key)
    }

    fn get_delta_chain(&self, key: &Key) -> Fallible<Vec<Delta>> {
        self.datastore.get_delta_chain(key)
    }

    fn get_meta(&self, key: &Key) -> Fallible<Metadata> {
        self.datastore.get_meta(key)
    }
}

impl LocalStore for ContentStore {
    fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>> {
        self.datastore.get_missing(keys)
    }
}

/// MutableDeltaStore is only implemented for the local store and not for the remote ones. The
/// remote stores will be automatically written to while calling the various `DataStore` methods.
impl MutableDeltaStore for ContentStore {
    /// Add the data to the local store.
    fn add(&self, delta: &Delta, metadata: &Metadata) -> Fallible<()> {
        self.local_mutabledatastore.add(delta, metadata)
    }

    /// Commit the data written to the local store.
    fn flush(&self) -> Fallible<Option<PathBuf>> {
        self.local_mutabledatastore.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use bytes::Bytes;
    use tempfile::TempDir;

    use types::testutil::*;

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

        let _store = ContentStore::new(&localdir, &config)?;
        Ok(())
    }

    #[test]
    fn test_add_get() -> Fallible<()> {
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
        assert_eq!(store.get_delta(&k1)?, delta);
        Ok(())
    }

    #[test]
    fn test_add_dropped() -> Fallible<()> {
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
        assert!(store.get_delta(&k1).is_err());
        Ok(())
    }

    #[test]
    fn test_add_flush_get() -> Fallible<()> {
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
        assert_eq!(store.get_delta(&k1)?, delta);
        Ok(())
    }

    #[test]
    fn test_add_flush_drop_get() -> Fallible<()> {
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
        assert_eq!(store.get_delta(&k1)?, delta);
        Ok(())
    }
}
