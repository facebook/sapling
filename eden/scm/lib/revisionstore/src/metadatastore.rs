/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::bail;
use anyhow::format_err;
use anyhow::Result;
use configmodel::Config;
use types::Key;
use types::NodeInfo;

use crate::historystore::HgIdHistoryStore;
use crate::historystore::HgIdMutableHistoryStore;
use crate::historystore::RemoteHistoryStore;
use crate::indexedloghistorystore::IndexedLogHgIdHistoryStore;
use crate::indexedlogutil::StoreType;
use crate::localstore::LocalStore;
use crate::remotestore::HgIdRemoteStore;
use crate::types::StoreKey;
use crate::unionhistorystore::UnionHgIdHistoryStore;
use crate::util::get_cache_path;
use crate::util::get_indexedloghistorystore_path;
use crate::util::get_local_path;
use crate::HistoryStore;

/// A `MetadataStore` aggregate all the local and remote stores and expose them as one. Both local and
/// remote stores can be queried and accessed via the `HgIdHistoryStore` trait. The local store can also
/// be written to via the `HgIdMutableHistoryStore` trait, this is intended to be used to store local
/// commit data.
pub struct MetadataStore {
    historystore: UnionHgIdHistoryStore<Arc<dyn HgIdHistoryStore>>,
    local_mutablehistorystore: Option<Arc<IndexedLogHgIdHistoryStore>>,
    shared_mutablehistorystore: Arc<IndexedLogHgIdHistoryStore>,
    remote_store: Option<Arc<dyn RemoteHistoryStore>>,
}

impl MetadataStore {
    pub fn new(local_path: impl AsRef<Path>, config: &dyn Config) -> Result<Self> {
        MetadataStoreBuilder::new(config)
            .local_path(&local_path)
            .build()
    }

    /// Attempt to repair the underlying stores that the `MetadataStore` is comprised of.
    ///
    /// As this may violate some of the stores assumptions, care must be taken to call this only
    /// when no other `MetadataStore` have been created for the `shared_path`.
    pub fn repair(
        shared_path: impl AsRef<Path>,
        local_path: Option<impl AsRef<Path>>,
        suffix: Option<impl AsRef<Path>>,
        config: &dyn Config,
    ) -> Result<String> {
        let mut repair_str = String::new();
        let mut shared_path = shared_path.as_ref().to_path_buf();
        if let Some(suffix) = suffix.as_ref() {
            shared_path.push(suffix);
        }
        let local_path = local_path
            .map(|p| get_local_path(p.as_ref().to_path_buf(), &suffix))
            .transpose()?;

        repair_str += &IndexedLogHgIdHistoryStore::repair(
            get_indexedloghistorystore_path(&shared_path)?,
            config,
            StoreType::Rotated,
        )?;
        if let Some(local_path) = local_path {
            repair_str += &IndexedLogHgIdHistoryStore::repair(
                get_indexedloghistorystore_path(local_path)?,
                config,
                StoreType::Permanent,
            )?;
        }
        Ok(repair_str)
    }
}

impl HgIdHistoryStore for MetadataStore {
    fn get_node_info(&self, key: &Key) -> Result<Option<NodeInfo>> {
        self.historystore.get_node_info(key)
    }

    fn refresh(&self) -> Result<()> {
        self.historystore.refresh()
    }
}

impl RemoteHistoryStore for MetadataStore {
    fn prefetch(&self, keys: &[StoreKey], length: Option<u32>) -> Result<()> {
        if let Some(remote_store) = self.remote_store.as_ref() {
            let missing = self.get_missing(keys)?;
            if missing == vec![] {
                Ok(())
            } else {
                remote_store.prefetch(&missing, length)
            }
        } else {
            // There is no remote store, let's pretend everything is fine.
            Ok(())
        }
    }
}

impl HistoryStore for MetadataStore {
    fn with_shared_only(&self) -> Arc<dyn HistoryStore> {
        let mut historystore: UnionHgIdHistoryStore<Arc<dyn HgIdHistoryStore>> =
            UnionHgIdHistoryStore::new();
        historystore.add(self.shared_mutablehistorystore.clone());
        if let Some(remote) = &self.remote_store {
            historystore.add(Arc::new(remote.clone()));
        }
        Arc::new(Self {
            historystore,
            shared_mutablehistorystore: self.shared_mutablehistorystore.clone(),
            remote_store: self.remote_store.clone(),
            local_mutablehistorystore: None,
        })
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

    fn flush(&self) -> Result<Option<Vec<PathBuf>>> {
        self.shared_mutablehistorystore.as_ref().flush()?;
        self.local_mutablehistorystore
            .as_ref()
            .ok_or_else(|| format_err!("flushing a non-local MetadataStore is not allowed"))?
            .flush()
    }
}

/// Builder for `MetadataStore`. An `impl AsRef<Path>` represents the path to the store and a
/// `dyn Config` of the Mercurial configuration are required to build a `MetadataStore`.
pub struct MetadataStoreBuilder<'a> {
    local_path: Option<PathBuf>,
    no_local_store: bool,
    config: &'a dyn Config,
    remotestore: Option<Arc<dyn HgIdRemoteStore>>,
    suffix: Option<PathBuf>,
}

impl<'a> MetadataStoreBuilder<'a> {
    pub fn new(config: &'a dyn Config) -> Self {
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

    pub fn remotestore(mut self, remotestore: Arc<dyn HgIdRemoteStore>) -> Self {
        self.remotestore = Some(remotestore);
        self
    }

    pub fn suffix(mut self, suffix: impl AsRef<Path>) -> Self {
        self.suffix = Some(suffix.as_ref().to_path_buf());
        self
    }

    pub fn build(self) -> Result<MetadataStore> {
        let local_path = self
            .local_path
            .as_ref()
            .map(|p| get_local_path(p.clone(), &self.suffix))
            .transpose()?;
        let cache_path = get_cache_path(self.config, &self.suffix)?;

        let mut historystore: UnionHgIdHistoryStore<Arc<dyn HgIdHistoryStore>> =
            UnionHgIdHistoryStore::new();

        let shared_indexedloghistorystore = match cache_path {
            Some(cache_path) => Some(Arc::new(IndexedLogHgIdHistoryStore::new(
                get_indexedloghistorystore_path(cache_path)?,
                &self.config,
                StoreType::Rotated,
            )?)),
            None => None,
        };

        // The shared store should precede the local one for 2 reasons:
        //  - It is expected that the number of blobs and the number of requests satisfied by the
        //    shared cache to be significantly higher than ones in the local store
        //  - When pushing changes on a pushrebase server, the local linknode will become
        //    incorrect, future fetches will put that change in the shared cache where the linknode
        //    will be correct.
        let primary: Option<Arc<IndexedLogHgIdHistoryStore>> = {
            // Put the indexedlog first, since recent data will have gone there.
            if let Some(shared_indexedloghistorystore) = shared_indexedloghistorystore.clone() {
                historystore.add(shared_indexedloghistorystore);
            }
            shared_indexedloghistorystore
        };

        let local_mutablehistorystore: Option<Arc<IndexedLogHgIdHistoryStore>> =
            if let Some(local_path) = local_path.as_ref() {
                let local_indexedloghistorystore = Arc::new(IndexedLogHgIdHistoryStore::new(
                    get_indexedloghistorystore_path(local_path)?,
                    &self.config,
                    StoreType::Permanent,
                )?);
                let primary: Arc<IndexedLogHgIdHistoryStore> = {
                    // Put the indexedlog first, since recent data will have gone there.
                    historystore.add(local_indexedloghistorystore.clone());
                    local_indexedloghistorystore
                };

                Some(primary)
            } else {
                if !self.no_local_store {
                    return Err(format_err!(
                        "a MetadataStore cannot be built without a local store"
                    ));
                }
                None
            };

        let primary = match primary {
            Some(primary) => primary,
            None => match local_mutablehistorystore.as_ref() {
                Some(local) => local.clone(),
                None => bail!("MetadataStore requires at least one of local store or shared store"),
            },
        };

        let remote_store: Option<Arc<dyn RemoteHistoryStore>> =
            if let Some(remotestore) = self.remotestore {
                let shared_store = primary.clone() as Arc<dyn HgIdMutableHistoryStore>;
                let remotestores = remotestore.historystore(shared_store);
                historystore.add(Arc::new(remotestores.clone()));
                Some(remotestores)
            } else {
                None
            };

        Ok(MetadataStore {
            historystore,
            local_mutablehistorystore,
            shared_mutablehistorystore: primary,
            remote_store,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tempfile::TempDir;
    use types::testutil::*;

    use super::*;
    use crate::testutil::make_config;
    use crate::testutil::FakeHgIdRemoteStore;

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
            .remotestore(Arc::new(remotestore))
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
            .remotestore(Arc::new(remotestore))
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
            .remotestore(Arc::new(remotestore))
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
            .remotestore(Arc::new(remotestore))
            .build()?;
        store.get_node_info(&k)?;
        assert_eq!(
            store.shared_mutablehistorystore.get_node_info(&k)?,
            Some(nodeinfo)
        );
        assert!(
            store
                .local_mutablehistorystore
                .as_ref()
                .unwrap()
                .get_node_info(&k)?
                .is_none()
        );
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

    #[test]
    fn test_local_indexedlog_write() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let store = MetadataStoreBuilder::new(&config)
            .local_path(&localdir)
            .build()?;

        let k1 = key("a", "1");
        let nodeinfo = NodeInfo {
            parents: [key("a", "2"), null_key("a")],
            linknode: hgid("3"),
        };

        store.add(&k1, &nodeinfo)?;
        store.flush()?;
        drop(store);

        let store = IndexedLogHgIdHistoryStore::new(
            get_indexedloghistorystore_path(&localdir)?,
            &config,
            StoreType::Permanent,
        )?;
        assert_eq!(store.get_node_info(&k1)?, Some(nodeinfo));
        Ok(())
    }
}
