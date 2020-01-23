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
    local_mutablehistorystore: Option<Box<dyn MutableHistoryStore>>,
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
        MetadataStoreBuilder::new(config)
            .local_path(&local_path)
            .build()
    }
}

impl HistoryStore for MetadataStore {
    fn get_node_info(&self, key: &Key) -> Result<Option<NodeInfo>> {
        self.inner.historystore.get_node_info(key)
    }
}

impl RemoteHistoryStore for MetadataStore {
    fn prefetch(&self, keys: &[Key]) -> Result<()> {
        if let Some(remote_store) = self.inner.remote_store.as_ref() {
            let missing = self.get_missing(&keys)?;
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
        self.inner
            .local_mutablehistorystore
            .as_ref()
            .ok_or_else(|| format_err!("writing to a non-local MetadataStore is not allowed"))?
            .add(key, info)
    }

    fn flush(&self) -> Result<Option<PathBuf>> {
        self.inner
            .local_mutablehistorystore
            .as_ref()
            .ok_or_else(|| format_err!("flushing a non-local MetadataStore is not allowed"))?
            .flush()
    }
}

/// Builder for `MetadataStore`. An `impl AsRef<Path>` represents the path to the store and a
/// `ConfigSet` of the Mercurial configuration are required to build a `MetadataStore`.
pub struct MetadataStoreBuilder<'a> {
    local_path: Option<PathBuf>,
    no_local_store: bool,
    config: &'a ConfigSet,
    remotestore: Option<Box<dyn RemoteStore>>,
    suffix: Option<&'a Path>,
}

impl<'a> MetadataStoreBuilder<'a> {
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

    /// Allows a MetadataStore to be created without a local store.
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

    pub fn build(self) -> Result<MetadataStore> {
        let cache_packs_path = get_cache_packs_path(self.config, self.suffix)?;
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

        // The shared store should precede the local one for 2 reasons:
        //  - It is expected that the number of blobs and the number of requests satisfied by the
        //    shared cache to be significantly higher than ones in the local store
        //  - When pushing changes on a pushrebase server, the local linknode will become
        //    incorrect, future fetches will put that change in the shared cache where the linknode
        //    will be correct.
        historystore.add(shared_pack_store.clone());

        let local_mutablehistorystore: Option<Box<dyn MutableHistoryStore>> =
            if let Some(local_path) = self.local_path {
                let local_pack_store = Box::new(MutableHistoryPackStore::new(
                    get_local_packs_path(local_path, self.suffix)?,
                    CorruptionPolicy::IGNORE,
                )?);
                historystore.add(local_pack_store.clone());

                Some(local_pack_store)
            } else {
                if !self.no_local_store {
                    return Err(format_err!(
                        "a MetadataStore cannot be built without a local store"
                    ));
                }
                None
            };

        let remote_store: Option<Arc<dyn RemoteHistoryStore>> =
            if let Some(remotestore) = self.remotestore {
                let store = remotestore.historystore(shared_pack_store.clone());
                historystore.add(Box::new(store.clone()));
                Some(store)
            } else {
                None
            };

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

        let store = MetadataStoreBuilder::new(&config)
            .local_path(&localdir)
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

        let store = MetadataStoreBuilder::new(&config)
            .local_path(&localdir)
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

        let store = MetadataStoreBuilder::new(&config)
            .local_path(&localdir)
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

        let store = MetadataStoreBuilder::new(&config)
            .local_path(&localdir)
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
            .as_ref()
            .unwrap()
            .get_node_info(&k)?
            .is_none());
        Ok(())
    }

    #[test]
    fn test_add_shared_only_store() -> Result<()> {
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

        let store = MetadataStoreBuilder::new(&config)
            .no_local_store()
            .build()?;
        assert_eq!(store.get_node_info(&k)?, None);
        Ok(())
    }

    #[test]
    fn test_no_local_store() -> Result<()> {
        let cachedir = TempDir::new()?;
        let config = make_config(&cachedir);
        assert!(MetadataStoreBuilder::new(&config).build().is_err());
        Ok(())
    }
}
