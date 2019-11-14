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

use failure::Fallible as Result;

use configparser::{config::ConfigSet, hg::ConfigSetHgExt};
use types::{Key, NodeInfo};

use crate::{
    historystore::{HistoryStore, MutableHistoryStore, RemoteHistoryStore},
    indexedloghistorystore::IndexedLogHistoryStore,
    localstore::LocalStore,
    packstore::{CorruptionPolicy, MutableHistoryPackStore},
    remotestore::RemoteStore,
    unionhistorystore::UnionHistoryStore,
    util::{get_cache_indexedloghistorystore_path, get_cache_packs_path, get_local_packs_path},
};

struct MetadataStoreInner {
    historystore: UnionHistoryStore<Box<dyn HistoryStore>>,
    local_mutablehistorystore: Box<dyn MutableHistoryStore>,
    shared_mutablehistorystore: Box<dyn MutableHistoryStore>,
    remote_store: Option<Arc<dyn RemoteHistoryStore>>,
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
    pub fn new(local_path: impl AsRef<Path>, config: &ConfigSet) -> Result<Self> {
        MetadataStoreBuilder::new(&local_path, config).build()
    }
}

impl HistoryStore for MetadataStore {
    fn get_node_info(&self, key: &Key) -> Result<Option<NodeInfo>> {
        self.inner.historystore.get_node_info(key)
    }
}

impl RemoteHistoryStore for MetadataStore {
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

impl LocalStore for MetadataStore {
    fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>> {
        self.inner.historystore.get_missing(keys)
    }
}

impl Drop for MetadataStoreInner {
    /// The shared store is a cache, so let's flush all pending data when the `MetadataStore` goes
    /// out of scope.
    fn drop(&mut self) {
        let _ = self.shared_mutablehistorystore.flush();
    }
}

impl MutableHistoryStore for MetadataStore {
    fn add(&self, key: &Key, info: &NodeInfo) -> Result<()> {
        self.inner.local_mutablehistorystore.add(key, info)
    }

    fn flush(&self) -> Result<Option<PathBuf>> {
        self.inner.local_mutablehistorystore.flush()
    }
}

/// Builder for `MetadataStore`. An `impl AsRef<Path>` represents the path to the store and a
/// `ConfigSet` of the Mercurial configuration are required to build a `MetadataStore`.
pub struct MetadataStoreBuilder<'a> {
    local_path: PathBuf,
    config: &'a ConfigSet,
    remotestore: Option<Box<dyn RemoteStore>>,
    suffix: Option<&'a Path>,
}

impl<'a> MetadataStoreBuilder<'a> {
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

    pub fn build(self) -> Result<MetadataStore> {
        let cache_packs_path = get_cache_packs_path(self.config, self.suffix)?;
        let local_pack_store = Box::new(MutableHistoryPackStore::new(
            get_local_packs_path(self.local_path, self.suffix)?,
            CorruptionPolicy::IGNORE,
        )?);
        let shared_pack_store = Box::new(MutableHistoryPackStore::new(
            &cache_packs_path,
            CorruptionPolicy::REMOVE,
        )?);
        let mut historystore: UnionHistoryStore<Box<dyn HistoryStore>> = UnionHistoryStore::new();

        if self
            .config
            .get_or_default::<bool>("remotefilelog", "indexedloghistorystore")?
        {
            let shared_indexedloghistorystore = Box::new(IndexedLogHistoryStore::new(
                get_cache_indexedloghistorystore_path(self.config)?,
            )?);
            historystore.add(shared_indexedloghistorystore);
        }

        historystore.add(shared_pack_store.clone());
        historystore.add(local_pack_store.clone());

        let remote_store: Option<Arc<dyn RemoteHistoryStore>> =
            if let Some(remotestore) = self.remotestore {
                let store = remotestore.historystore(shared_pack_store.clone());
                historystore.add(Box::new(store.clone()));
                Some(store)
            } else {
                None
            };

        let local_mutablehistorystore: Box<dyn MutableHistoryStore> = local_pack_store;
        let shared_mutablehistorystore: Box<dyn MutableHistoryStore> = shared_pack_store;

        Ok(MetadataStore {
            inner: Arc::new(MetadataStoreInner {
                historystore,
                local_mutablehistorystore,
                shared_mutablehistorystore,
                remote_store,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

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

        MetadataStore::new(&localdir, &config)?;
        Ok(())
    }

    #[test]
    fn test_add_get() -> Result<()> {
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
    fn test_add_dropped() -> Result<()> {
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
    fn test_add_flush_get() -> Result<()> {
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
    fn test_add_flush_drop_get() -> Result<()> {
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

    #[test]
    fn test_remote_store() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let k = key("a", "1");
        let nodeinfo = NodeInfo {
            parents: [key("a", "2"), null_key("a")],
            linknode: hgid("3"),
        };

        let mut map = HashMap::new();
        map.insert(k.clone(), nodeinfo.clone());
        let mut remotestore = FakeRemoteStore::new();
        remotestore.hist(map);

        let store = MetadataStoreBuilder::new(&localdir, &config)
            .remotestore(Box::new(remotestore))
            .build()?;
        assert_eq!(store.get_node_info(&k)?, Some(nodeinfo));
        Ok(())
    }

    #[test]
    fn test_remote_store_cached() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let k = key("a", "1");
        let nodeinfo = NodeInfo {
            parents: [key("a", "2"), null_key("a")],
            linknode: hgid("3"),
        };

        let mut map = HashMap::new();
        map.insert(k.clone(), nodeinfo.clone());
        let mut remotestore = FakeRemoteStore::new();
        remotestore.hist(map);

        let store = MetadataStoreBuilder::new(&localdir, &config)
            .remotestore(Box::new(remotestore))
            .build()?;
        store.get_node_info(&k)?;
        drop(store);

        let store = MetadataStore::new(&localdir, &config)?;
        assert_eq!(store.get_node_info(&k)?, Some(nodeinfo));
        Ok(())
    }

    #[test]
    fn test_not_in_remote_store() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let map = HashMap::new();
        let mut remotestore = FakeRemoteStore::new();
        remotestore.hist(map);

        let store = MetadataStoreBuilder::new(&localdir, &config)
            .remotestore(Box::new(remotestore))
            .build()?;

        let k = key("a", "1");
        assert_eq!(store.get_node_info(&k)?, None);
        Ok(())
    }

    #[test]
    fn test_fetch_location() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let k = key("a", "1");
        let nodeinfo = NodeInfo {
            parents: [key("a", "2"), null_key("a")],
            linknode: hgid("3"),
        };

        let mut map = HashMap::new();
        map.insert(k.clone(), nodeinfo.clone());
        let mut remotestore = FakeRemoteStore::new();
        remotestore.hist(map);

        let store = MetadataStoreBuilder::new(&localdir, &config)
            .remotestore(Box::new(remotestore))
            .build()?;
        store.get_node_info(&k)?;
        assert_eq!(
            store.inner.shared_mutablehistorystore.get_node_info(&k)?,
            Some(nodeinfo)
        );
        assert!(store
            .inner
            .local_mutablehistorystore
            .get_node_info(&k)?
            .is_none());
        Ok(())
    }
}
