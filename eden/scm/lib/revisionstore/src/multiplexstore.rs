/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use anyhow::Result;
use types::Key;
use types::NodeInfo;

use crate::datastore::Delta;
use crate::datastore::HgIdDataStore;
use crate::datastore::HgIdMutableDeltaStore;
use crate::datastore::Metadata;
use crate::datastore::StoreResult;
use crate::historystore::HgIdHistoryStore;
use crate::historystore::HgIdMutableHistoryStore;
use crate::localstore::LocalStore;
use crate::types::StoreKey;

/// A `MultiplexDeltaStore` is a store that will duplicate all the writes to all the
/// delta stores that it is made of.
pub struct MultiplexDeltaStore<T: HgIdMutableDeltaStore> {
    stores: Vec<T>,
}

/// A `MultiplexHgIdHistoryStore` is a store that will duplicate all the writes to all the
/// history stores that it is made of.
pub struct MultiplexHgIdHistoryStore<T: HgIdMutableHistoryStore> {
    stores: Vec<T>,
}

impl<T: HgIdMutableDeltaStore> MultiplexDeltaStore<T> {
    pub fn new() -> Self {
        Self { stores: Vec::new() }
    }

    pub fn add_store(&mut self, store: T) {
        self.stores.push(store)
    }
}

impl<T: HgIdMutableHistoryStore> MultiplexHgIdHistoryStore<T> {
    pub fn new() -> Self {
        Self { stores: Vec::new() }
    }

    pub fn add_store(&mut self, store: T) {
        self.stores.push(store)
    }
}

impl<T: HgIdMutableDeltaStore> HgIdMutableDeltaStore for MultiplexDeltaStore<T> {
    /// Write the `Delta` and `Metadata` to all the stores
    fn add(&self, delta: &Delta, metadata: &Metadata) -> Result<()> {
        for store in self.stores.iter() {
            store.add(delta, metadata)?;
        }

        Ok(())
    }

    fn flush(&self) -> Result<Option<Vec<PathBuf>>> {
        let mut ret = None;
        for store in self.stores.iter() {
            let opt = store.flush()?;
            // It's non sensical for the MultiplexStore to be built with multiple pack stores,
            // therefore let's assert that only one store can ever return a PathBuf.
            assert!(opt.is_none() || !ret.is_some());
            ret = ret.or(opt);
        }

        Ok(ret)
    }
}

impl<T: HgIdMutableDeltaStore> HgIdDataStore for MultiplexDeltaStore<T> {
    fn get(&self, mut key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        for store in self.stores.iter() {
            match store.get(key)? {
                StoreResult::Found(data) => return Ok(StoreResult::Found(data)),
                StoreResult::NotFound(next) => key = next,
            }
        }

        Ok(StoreResult::NotFound(key))
    }

    fn get_meta(&self, mut key: StoreKey) -> Result<StoreResult<Metadata>> {
        for store in self.stores.iter() {
            match store.get_meta(key)? {
                StoreResult::Found(data) => return Ok(StoreResult::Found(data)),
                StoreResult::NotFound(next) => key = next,
            }
        }

        Ok(StoreResult::NotFound(key))
    }

    fn refresh(&self) -> Result<()> {
        for store in self.stores.iter() {
            store.refresh()?;
        }
        Ok(())
    }
}

impl<T: HgIdMutableDeltaStore> LocalStore for MultiplexDeltaStore<T> {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        let initial_keys = Ok(keys.to_vec());
        self.stores
            .iter()
            .fold(initial_keys, |missing_keys, store| match missing_keys {
                Ok(missing_keys) => store.get_missing(&missing_keys),
                Err(e) => Err(e),
            })
    }
}

impl<T: HgIdMutableHistoryStore> HgIdMutableHistoryStore for MultiplexHgIdHistoryStore<T> {
    fn add(&self, key: &Key, info: &NodeInfo) -> Result<()> {
        for store in self.stores.iter() {
            store.add(key, info)?;
        }

        Ok(())
    }

    fn flush(&self) -> Result<Option<Vec<PathBuf>>> {
        let mut ret = None;
        for store in self.stores.iter() {
            let opt = store.flush()?;
            // It's non sensical for the MultiplexStore to be built with multiple pack stores,
            // therefore let's assert that only one store can ever return a PathBuf.
            assert!(opt.is_none() || !ret.is_some());
            ret = ret.or(opt);
        }

        Ok(ret)
    }
}

impl<T: HgIdMutableHistoryStore> HgIdHistoryStore for MultiplexHgIdHistoryStore<T> {
    fn get_node_info(&self, key: &Key) -> Result<Option<NodeInfo>> {
        for store in self.stores.iter() {
            if let Some(nodeinfo) = store.get_node_info(key)? {
                return Ok(Some(nodeinfo));
            }
        }

        Ok(None)
    }

    fn refresh(&self) -> Result<()> {
        for store in self.stores.iter() {
            store.refresh()?;
        }
        Ok(())
    }
}

impl<T: HgIdMutableHistoryStore> LocalStore for MultiplexHgIdHistoryStore<T> {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        let initial_keys = Ok(keys.to_vec());
        self.stores
            .iter()
            .fold(initial_keys, |missing_keys, store| match missing_keys {
                Ok(missing_keys) => store.get_missing(&missing_keys),
                Err(e) => Err(e),
            })
    }
}

#[cfg(test)]
mod tests {
    use minibytes::Bytes;
    use tempfile::TempDir;
    use types::testutil::*;

    use super::*;
    use crate::datapack::DataPackVersion;
    use crate::datastore::HgIdDataStore;
    use crate::historypack::HistoryPackVersion;
    use crate::historystore::HgIdHistoryStore;
    use crate::indexedlogdatastore::IndexedLogHgIdDataStore;
    use crate::indexedlogdatastore::IndexedLogHgIdDataStoreConfig;
    use crate::indexedlogutil::StoreType;
    use crate::localstore::ExtStoredPolicy;
    use crate::mutabledatapack::MutableDataPack;
    use crate::mutablehistorypack::MutableHistoryPack;

    #[test]
    fn test_delta_add_static() -> Result<()> {
        let tempdir = TempDir::new()?;
        let config = IndexedLogHgIdDataStoreConfig {
            max_log_count: None,
            max_bytes_per_log: None,
            max_bytes: None,
        };
        let mut log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Ignore,
            &config,
            StoreType::Shared,
        )?;
        let mut multiplex = MultiplexDeltaStore::new();
        multiplex.add_store(Box::new(&mut log));

        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: key("a", "1"),
        };
        let metadata = Default::default();

        multiplex.add(&delta, &metadata)?;
        drop(multiplex);
        let read_data = log.get(StoreKey::hgid(delta.key))?;
        assert_eq!(StoreResult::Found(delta.data.as_ref().to_vec()), read_data);
        log.flush()?;
        Ok(())
    }

    #[test]
    fn test_delta_add_dynamic() -> Result<()> {
        let tempdir = TempDir::new()?;
        let config = IndexedLogHgIdDataStoreConfig {
            max_log_count: None,
            max_bytes_per_log: None,
            max_bytes: None,
        };
        let mut log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Ignore,
            &config,
            StoreType::Shared,
        )?;
        let mut pack = MutableDataPack::new(&tempdir, DataPackVersion::One);
        let mut multiplex: MultiplexDeltaStore<Box<dyn HgIdMutableDeltaStore>> =
            MultiplexDeltaStore::new();
        multiplex.add_store(Box::new(&mut log));
        multiplex.add_store(Box::new(&mut pack));

        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: key("a", "1"),
        };
        let metadata = Default::default();

        multiplex.add(&delta, &metadata)?;
        drop(multiplex);

        let k = StoreKey::hgid(delta.key);
        let read_data = log.get(k.clone())?;
        assert_eq!(StoreResult::Found(delta.data.as_ref().to_vec()), read_data);

        let read_data = pack.get(k)?;
        assert_eq!(StoreResult::Found(delta.data.as_ref().to_vec()), read_data);

        log.flush()?;
        pack.flush()?;
        Ok(())
    }

    #[test]
    fn test_history_add_static() -> Result<()> {
        let tempdir = TempDir::new()?;
        let mut pack = MutableHistoryPack::new(&tempdir, HistoryPackVersion::One);
        let mut multiplex = MultiplexHgIdHistoryStore::new();
        multiplex.add_store(Box::new(&mut pack));

        let k = key("a", "1");
        let nodeinfo = NodeInfo {
            parents: [key("a", "2"), key("a", "3")],
            linknode: hgid("4"),
        };

        multiplex.add(&k, &nodeinfo)?;
        drop(multiplex);

        let read_hgid = pack.get_node_info(&k)?;
        assert_eq!(Some(nodeinfo), read_hgid);

        pack.flush()?;
        Ok(())
    }

    #[test]
    fn test_history_add_dynamic() -> Result<()> {
        let tempdir = TempDir::new()?;
        let mut pack1 = MutableHistoryPack::new(&tempdir, HistoryPackVersion::One);
        let mut pack2 = MutableHistoryPack::new(&tempdir, HistoryPackVersion::One);
        let mut multiplex = MultiplexHgIdHistoryStore::new();
        multiplex.add_store(Box::new(&mut pack1));
        multiplex.add_store(Box::new(&mut pack2));

        let k = key("a", "1");
        let nodeinfo = NodeInfo {
            parents: [key("a", "2"), key("a", "3")],
            linknode: hgid("4"),
        };

        multiplex.add(&k, &nodeinfo)?;
        drop(multiplex);

        let read_hgid = pack1.get_node_info(&k)?;
        assert_eq!(Some(nodeinfo.clone()), read_hgid);

        let read_hgid = pack2.get_node_info(&k)?;
        assert_eq!(Some(nodeinfo), read_hgid);

        pack1.flush()?;
        pack2.flush()?;
        Ok(())
    }
}
