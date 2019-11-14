/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use failure::{format_err, Fallible as Result};

use types::{Key, NodeInfo};

use crate::datastore::{DataStore, Delta, Metadata, MutableDeltaStore};
use crate::historystore::{HistoryStore, MutableHistoryStore};
use crate::localstore::LocalStore;

/// A `MultiplexDeltaStore` is a store that will duplicate all the writes to all the
/// delta stores that it is made of.
pub struct MultiplexDeltaStore<'a> {
    stores: Vec<Box<dyn MutableDeltaStore + Send + 'a>>,
}

/// A `MultiplexHistoryStore` is a store that will duplicate all the writes to all the
/// history stores that it is made of.
pub struct MultiplexHistoryStore<'a> {
    stores: Vec<Box<dyn MutableHistoryStore + Send + 'a>>,
}

impl<'a> MultiplexDeltaStore<'a> {
    pub fn new() -> Self {
        Self { stores: Vec::new() }
    }

    pub fn add_store(&mut self, store: Box<dyn MutableDeltaStore + Send + 'a>) {
        self.stores.push(store)
    }
}

impl<'a> MultiplexHistoryStore<'a> {
    pub fn new() -> Self {
        Self { stores: Vec::new() }
    }

    pub fn add_store(&mut self, store: Box<dyn MutableHistoryStore + Send + 'a>) {
        self.stores.push(store)
    }
}

impl<'a> MutableDeltaStore for MultiplexDeltaStore<'a> {
    /// Write the `Delta` and `Metadata` to all the stores
    fn add(&self, delta: &Delta, metadata: &Metadata) -> Result<()> {
        for store in self.stores.iter() {
            store.add(delta, metadata)?;
        }

        Ok(())
    }

    fn flush(&self) -> Result<Option<PathBuf>> {
        for store in self.stores.iter() {
            store.flush()?;
        }

        Ok(None)
    }
}

impl<'a> DataStore for MultiplexDeltaStore<'a> {
    fn get(&self, _key: &Key) -> Result<Option<Vec<u8>>> {
        Err(format_err!("MultiplexDeltaStore doesn't support raw get()"))
    }

    fn get_delta(&self, key: &Key) -> Result<Option<Delta>> {
        for store in self.stores.iter() {
            if let Some(result) = store.get_delta(key)? {
                return Ok(Some(result));
            }
        }

        Ok(None)
    }

    fn get_delta_chain(&self, key: &Key) -> Result<Option<Vec<Delta>>> {
        for store in self.stores.iter() {
            if let Some(result) = store.get_delta_chain(key)? {
                return Ok(Some(result));
            }
        }

        Ok(None)
    }

    fn get_meta(&self, key: &Key) -> Result<Option<Metadata>> {
        for store in self.stores.iter() {
            if let Some(result) = store.get_meta(key)? {
                return Ok(Some(result));
            }
        }

        Ok(None)
    }
}

impl<'a> LocalStore for MultiplexDeltaStore<'a> {
    fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>> {
        let initial_keys = Ok(keys.iter().cloned().collect());
        self.stores
            .iter()
            .fold(initial_keys, |missing_keys, store| match missing_keys {
                Ok(missing_keys) => store.get_missing(&missing_keys),
                Err(e) => Err(e),
            })
    }
}

impl<'a> MutableHistoryStore for MultiplexHistoryStore<'a> {
    fn add(&self, key: &Key, info: &NodeInfo) -> Result<()> {
        for store in self.stores.iter() {
            store.add(key, info)?;
        }

        Ok(())
    }

    fn flush(&self) -> Result<Option<PathBuf>> {
        for store in self.stores.iter() {
            store.flush()?;
        }

        Ok(None)
    }
}

impl<'a> HistoryStore for MultiplexHistoryStore<'a> {
    fn get_node_info(&self, key: &Key) -> Result<Option<NodeInfo>> {
        for store in self.stores.iter() {
            if let Some(nodeinfo) = store.get_node_info(key)? {
                return Ok(Some(nodeinfo));
            }
        }

        Ok(None)
    }
}

impl<'a> LocalStore for MultiplexHistoryStore<'a> {
    fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>> {
        let initial_keys = Ok(keys.iter().cloned().collect());
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
    use super::*;

    use bytes::Bytes;
    use tempfile::TempDir;

    use types::testutil::*;

    use crate::datapack::DataPackVersion;
    use crate::datastore::DataStore;
    use crate::historypack::HistoryPackVersion;
    use crate::historystore::HistoryStore;
    use crate::indexedlogdatastore::IndexedLogDataStore;
    use crate::mutabledatapack::MutableDataPack;
    use crate::mutablehistorypack::MutableHistoryPack;

    #[test]
    fn test_delta_add_static() -> Result<()> {
        let tempdir = TempDir::new()?;
        let mut log = IndexedLogDataStore::new(&tempdir)?;
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
        let read_delta = log.get_delta(&delta.key)?;
        assert_eq!(Some(delta), read_delta);
        log.flush()?;
        Ok(())
    }

    #[test]
    fn test_delta_add_dynamic() -> Result<()> {
        let tempdir = TempDir::new()?;
        let mut log = IndexedLogDataStore::new(&tempdir)?;
        let mut pack = MutableDataPack::new(&tempdir, DataPackVersion::One)?;
        let mut multiplex = MultiplexDeltaStore::new();
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

        let read_delta = log.get_delta(&delta.key)?;
        assert_eq!(Some(delta.clone()), read_delta);

        let read_delta = pack.get_delta(&delta.key)?;
        assert_eq!(Some(delta), read_delta);

        log.flush()?;
        pack.flush()?;
        Ok(())
    }

    #[test]
    fn test_history_add_static() -> Result<()> {
        let tempdir = TempDir::new()?;
        let mut pack = MutableHistoryPack::new(&tempdir, HistoryPackVersion::One)?;
        let mut multiplex = MultiplexHistoryStore::new();
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
        let mut pack1 = MutableHistoryPack::new(&tempdir, HistoryPackVersion::One)?;
        let mut pack2 = MutableHistoryPack::new(&tempdir, HistoryPackVersion::One)?;
        let mut multiplex = MultiplexHistoryStore::new();
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
