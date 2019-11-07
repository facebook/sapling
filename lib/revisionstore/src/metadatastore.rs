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

use failure::Fallible;

use configparser::config::ConfigSet;
use types::{Key, NodeInfo};

use crate::{
    historystore::{HistoryStore, MutableHistoryStore},
    indexedloghistorystore::IndexedLogHistoryStore,
    localstore::LocalStore,
    packstore::{CorruptionPolicy, MutableHistoryPackStore},
    unionhistorystore::UnionHistoryStore,
    util::{get_cache_indexedloghistorystore_path, get_cache_packs_path, get_local_packs_path},
};

struct MetadataStoreInner {
    historystore: UnionHistoryStore<Box<dyn HistoryStore>>,
    local_mutablehistorystore: Box<dyn MutableHistoryStore>,
    shared_mutablehistorystore: Box<dyn MutableHistoryStore>,
}

/// A `MetadataStore` aggregate all the local and remote stores and expose them as one. Both local and
/// remote stores can be queried and accessed via the `HistoryStore` trait. The local store can also
/// be written to via the `MutableHistoryStore` trait, this is intended to be used to store local
/// commit data.
#[derive(Clone)]
pub struct MetadataStore {
    inner: Arc<MetadataStoreInner>,
}

impl MetadataStore {
    pub fn new(local_path: impl AsRef<Path>, config: &ConfigSet) -> Fallible<Self> {
        MetadataStoreBuilder::new(&local_path, config).build()
    }
}

impl HistoryStore for MetadataStore {
    fn get_node_info(&self, key: &Key) -> Fallible<Option<NodeInfo>> {
        self.inner.historystore.get_node_info(key)
    }
}

impl LocalStore for MetadataStore {
    fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>> {
        self.inner.historystore.get_missing(keys)
    }
}

impl Drop for MetadataStore {
    /// The shared store is a cache, so let's flush all pending data when the `MetadataStore` goes
    /// out of scope.
    fn drop(&mut self) {
        let _ = self.inner.shared_mutablehistorystore.flush();
    }
}

impl MutableHistoryStore for MetadataStore {
    fn add(&self, key: &Key, info: &NodeInfo) -> Fallible<()> {
        self.inner.local_mutablehistorystore.add(key, info)
    }

    fn flush(&self) -> Fallible<Option<PathBuf>> {
        self.inner.local_mutablehistorystore.flush()
    }
}

/// Builder for `MetadataStore`. An `impl AsRef<Path>` represents the path to the store and a
/// `ConfigSet` of the Mercurial configuration are required to build a `MetadataStore`.
pub struct MetadataStoreBuilder<'a> {
    local_path: PathBuf,
    config: &'a ConfigSet,
    suffix: Option<&'a Path>,
}

impl<'a> MetadataStoreBuilder<'a> {
    pub fn new(local_path: impl AsRef<Path>, config: &'a ConfigSet) -> Self {
        Self {
            local_path: local_path.as_ref().to_path_buf(),
            config,
            suffix: None,
        }
    }

    pub fn suffix(mut self, suffix: &'a Path) -> Self {
        self.suffix = Some(suffix);
        self
    }

    pub fn build(self) -> Fallible<MetadataStore> {
        let cache_packs_path = get_cache_packs_path(self.config, self.suffix)?;
        let local_pack_store = Box::new(MutableHistoryPackStore::new(
            get_local_packs_path(self.local_path, self.suffix)?,
            CorruptionPolicy::IGNORE,
        )?);
        let shared_pack_store = Box::new(MutableHistoryPackStore::new(
            &cache_packs_path,
            CorruptionPolicy::REMOVE,
        )?);
        let shared_indexedloghistorystore = Box::new(IndexedLogHistoryStore::new(
            get_cache_indexedloghistorystore_path(self.config)?,
        )?);

        let mut historystore: UnionHistoryStore<Box<dyn HistoryStore>> = UnionHistoryStore::new();

        historystore.add(shared_indexedloghistorystore);
        historystore.add(shared_pack_store.clone());
        historystore.add(local_pack_store.clone());

        let local_mutablehistorystore: Box<dyn MutableHistoryStore> = local_pack_store;
        let shared_mutablehistorystore: Box<dyn MutableHistoryStore> = shared_pack_store;

        Ok(MetadataStore {
            inner: Arc::new(MetadataStoreInner {
                historystore,
                local_mutablehistorystore,
                shared_mutablehistorystore,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        MetadataStore::new(&localdir, &config)?;
        Ok(())
    }

    #[test]
    fn test_add_get() -> Fallible<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let store = MetadataStore::new(&localdir, &config)?;

        let k = key("a", "1");
        let nodeinfo = NodeInfo {
            parents: [key("a", "2"), null_key("a")],
            linknode: hgid("3"),
        };

        store.add(&k, &nodeinfo)?;
        assert_eq!(store.get_node_info(&k)?, Some(nodeinfo));
        Ok(())
    }

    #[test]
    fn test_add_dropped() -> Fallible<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let store = MetadataStore::new(&localdir, &config)?;

        let k = key("a", "1");
        let nodeinfo = NodeInfo {
            parents: [key("a", "2"), null_key("a")],
            linknode: hgid("3"),
        };

        store.add(&k, &nodeinfo)?;
        drop(store);

        let store = MetadataStore::new(&localdir, &config)?;
        assert!(store.get_node_info(&k)?.is_none());
        Ok(())
    }

    #[test]
    fn test_add_flush_get() -> Fallible<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let store = MetadataStore::new(&localdir, &config)?;

        let k = key("a", "1");
        let nodeinfo = NodeInfo {
            parents: [key("a", "2"), null_key("a")],
            linknode: hgid("3"),
        };

        store.add(&k, &nodeinfo)?;
        store.flush()?;
        assert_eq!(store.get_node_info(&k)?, Some(nodeinfo));
        Ok(())
    }

    #[test]
    fn test_add_flush_drop_get() -> Fallible<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let store = MetadataStore::new(&localdir, &config)?;

        let k = key("a", "1");
        let nodeinfo = NodeInfo {
            parents: [key("a", "2"), null_key("a")],
            linknode: hgid("3"),
        };

        store.add(&k, &nodeinfo)?;
        store.flush()?;
        drop(store);

        let store = MetadataStore::new(&localdir, &config)?;
        assert_eq!(store.get_node_info(&k)?, Some(nodeinfo));
        Ok(())
    }
}
