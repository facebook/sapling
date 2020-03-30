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
    historystore::{HgIdHistoryStore, HgIdMutableHistoryStore, RemoteHistoryStore},
    indexedloghistorystore::IndexedLogHgIdHistoryStore,
    localstore::LocalStore,
    memcache::MemcacheStore,
    multiplexstore::MultiplexHgIdHistoryStore,
    packstore::{CorruptionPolicy, MutableHistoryPackStore},
    remotestore::HgIdRemoteStore,
    repack::RepackLocation,
    types::StoreKey,
    unionhistorystore::UnionHgIdHistoryStore,
    util::{
        get_cache_packs_path, get_cache_path, get_indexedloghistorystore_path, get_local_path,
        get_packs_path,
    },
};

/// A `MetadataStore` aggregate all the local and remote stores and expose them as one. Both local and
/// remote stores can be queried and accessed via the `HgIdHistoryStore` trait. The local store can also
/// be written to via the `HgIdMutableHistoryStore` trait, this is intended to be used to store local
/// commit data.
pub struct MetadataStore {
    historystore: UnionHgIdHistoryStore<Arc<dyn HgIdHistoryStore>>,
    local_mutablehistorystore: Option<Arc<dyn HgIdMutableHistoryStore>>,
    shared_mutablehistorystore: Arc<dyn HgIdMutableHistoryStore>,
    remote_store: Option<Arc<dyn RemoteHistoryStore>>,
}

impl MetadataStore {
    pub fn new(local_path: impl AsRef<Path>, config: &ConfigSet) -> Result<Self> {
        MetadataStoreBuilder::new(config)
            .local_path(&local_path)
            .build()
    }
}

// Repack specific methods, not to be used directly but by the repack code.
impl MetadataStore {
    pub(crate) fn add_pending(
        &self,
        key: &Key,
        info: NodeInfo,
        location: RepackLocation,
    ) -> Result<()> {
        match location {
            RepackLocation::Local => self.add(&key, &info),
            RepackLocation::Shared => self.shared_mutablehistorystore.add(&key, &info),
        }
    }

    pub(crate) fn commit_pending(&self, location: RepackLocation) -> Result<Option<PathBuf>> {
        match location {
            RepackLocation::Local => self.flush(),
            RepackLocation::Shared => self.shared_mutablehistorystore.flush(),
        }
    }
}

impl HgIdHistoryStore for MetadataStore {
    fn get_node_info(&self, key: &Key) -> Result<Option<NodeInfo>> {
        self.historystore.get_node_info(key)
    }
}

impl RemoteHistoryStore for MetadataStore {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<()> {
        if let Some(remote_store) = self.remote_store.as_ref() {
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
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.historystore.get_missing(keys)
    }
}

impl Drop for MetadataStore {
    /// The shared store is a cache, so let's flush all pending data when the `MetadataStore` goes
    /// out of scope.
    fn drop(&mut self) {
        let _ = self.shared_mutablehistorystore.flush();
    }
}

impl HgIdMutableHistoryStore for MetadataStore {
    fn add(&self, key: &Key, info: &NodeInfo) -> Result<()> {
        self.local_mutablehistorystore
            .as_ref()
            .ok_or_else(|| format_err!("writing to a non-local MetadataStore is not allowed"))?
            .add(key, info)
    }

    fn flush(&self) -> Result<Option<PathBuf>> {
        self.local_mutablehistorystore
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
    remotestore: Option<Box<dyn HgIdRemoteStore>>,
    suffix: Option<PathBuf>,
    memcachestore: Option<MemcacheStore>,
}

impl<'a> MetadataStoreBuilder<'a> {
    pub fn new(config: &'a ConfigSet) -> Self {
        Self {
            local_path: None,
            no_local_store: false,
            config,
            remotestore: None,
            suffix: None,
            memcachestore: None,
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

    pub fn remotestore(mut self, remotestore: Box<dyn HgIdRemoteStore>) -> Self {
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

    pub fn build(self) -> Result<MetadataStore> {
        let _local_path = get_local_path(&self.local_path, &self.suffix)?;
        let cache_path = get_cache_path(self.config, &self.suffix)?;

        let cache_packs_path = get_cache_packs_path(self.config, &self.suffix)?;
        let shared_pack_store = Arc::new(MutableHistoryPackStore::new(
            &cache_packs_path,
            CorruptionPolicy::REMOVE,
        )?);
        let mut historystore: UnionHgIdHistoryStore<Arc<dyn HgIdHistoryStore>> =
            UnionHgIdHistoryStore::new();

        if self
            .config
            .get_or_default::<bool>("remotefilelog", "indexedloghistorystore")?
        {
            let shared_indexedloghistorystore = Arc::new(IndexedLogHgIdHistoryStore::new(
                get_indexedloghistorystore_path(&cache_path)?,
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

        let local_mutablehistorystore: Option<Arc<dyn HgIdMutableHistoryStore>> =
            if let Some(local_path) = self.local_path {
                let local_pack_store = Arc::new(MutableHistoryPackStore::new(
                    get_packs_path(&local_path, &self.suffix)?,
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

        let remote_store: Option<Arc<dyn RemoteHistoryStore>> = if let Some(remotestore) =
            self.remotestore
        {
            let (cache, shared_store) = if let Some(memcachestore) = self.memcachestore {
                // Combine the memcache store with the other stores. The intent is that all remote
                // requests will first go to the memcache store, and only reach the slower remote
                // store after that.
                //
                // If data isn't found in the memcache store, once fetched from the remote store it
                // will be written to the local cache, and will populate the memcache store, so
                // other clients and future requests won't need to go to a network store.
                let memcachehistorystore = memcachestore.historystore(shared_pack_store.clone());

                let mut multiplexstore: MultiplexHgIdHistoryStore<
                    Arc<dyn HgIdMutableHistoryStore>,
                > = MultiplexHgIdHistoryStore::new();
                multiplexstore.add_store(Arc::new(memcachestore));
                multiplexstore.add_store(shared_pack_store.clone());

                (
                    Some(memcachehistorystore),
                    Arc::new(multiplexstore) as Arc<dyn HgIdMutableHistoryStore>,
                )
            } else {
                (
                    None,
                    shared_pack_store.clone() as Arc<dyn HgIdMutableHistoryStore>,
                )
            };

            let store = remotestore.historystore(shared_store);

            let remotestores = if let Some(cache) = cache {
                let mut remotestores = UnionHgIdHistoryStore::new();
                remotestores.add(cache.clone());
                remotestores.add(store.clone());
                Arc::new(remotestores)
            } else {
                store
            };

            historystore.add(Arc::new(remotestores.clone()));
            Some(remotestores)
        } else {
            None
        };

        let shared_mutablehistorystore: Arc<dyn HgIdMutableHistoryStore> = shared_pack_store;

        Ok(MetadataStore {
            historystore,
            local_mutablehistorystore,
            shared_mutablehistorystore,
            remote_store,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    use tempfile::TempDir;

    use types::testutil::*;

    use crate::testutil::{make_config, FakeHgIdRemoteStore};

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
        let mut remotestore = FakeHgIdRemoteStore::new();
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
        let mut remotestore = FakeHgIdRemoteStore::new();
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
        let mut remotestore = FakeHgIdRemoteStore::new();
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
        let mut remotestore = FakeHgIdRemoteStore::new();
        remotestore.hist(map);

        let store = MetadataStoreBuilder::new(&config)
            .local_path(&localdir)
            .remotestore(Box::new(remotestore))
            .build()?;
        store.get_node_info(&k)?;
        assert_eq!(
            store.shared_mutablehistorystore.get_node_info(&k)?,
            Some(nodeinfo)
        );
        assert!(store
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

    #[cfg(fbcode_build)]
    mod fbcode_tests {
        use super::*;

        use memcache::MockMemcache;

        use once_cell::sync::Lazy;

        static MOCK: Lazy<MockMemcache> = Lazy::new(|| MockMemcache::new());

        #[fbinit::test]
        fn test_memcache_get() -> Result<()> {
            let _mock = Lazy::force(&MOCK);

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
            let mut remotestore = FakeHgIdRemoteStore::new();
            remotestore.hist(map);

            let memcache = MemcacheStore::new(&config)?;
            let store = MetadataStoreBuilder::new(&config)
                .local_path(&localdir)
                .remotestore(Box::new(remotestore))
                .memcachestore(memcache.clone())
                .build()?;
            let nodeinfo_get = store.get_node_info(&k)?;
            assert_eq!(nodeinfo_get, Some(nodeinfo.clone()));

            loop {
                let memcache_nodeinfo = memcache.get_node_info(&k)?;
                if let Some(memcache_nodeinfo) = memcache_nodeinfo {
                    assert_eq!(memcache_nodeinfo, nodeinfo);
                    break;
                }
            }

            let memcache_nodeinfo = memcache.get_node_info(&k)?;
            assert_eq!(memcache_nodeinfo, Some(nodeinfo));
            Ok(())
        }
    }
}
