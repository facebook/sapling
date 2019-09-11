// Copyright Facebook, Inc. 2019

use std::path::PathBuf;

use failure::{format_err, Fallible};

use types::{Key, NodeInfo};

use crate::datastore::{DataStore, Delta, Metadata, MutableDeltaStore};
use crate::error::KeyError;
use crate::historystore::{Ancestors, HistoryStore, MutableHistoryStore};
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
    fn add(&self, delta: &Delta, metadata: &Metadata) -> Fallible<()> {
        for store in self.stores.iter() {
            store.add(delta, metadata)?;
        }

        Ok(())
    }

    fn flush(&self) -> Fallible<Option<PathBuf>> {
        for store in self.stores.iter() {
            store.flush()?;
        }

        Ok(None)
    }
}

impl<'a> DataStore for MultiplexDeltaStore<'a> {
    fn get(&self, _key: &Key) -> Fallible<Vec<u8>> {
        Err(format_err!("MultiplexDeltaStore doesn't support raw get()"))
    }

    fn get_delta(&self, key: &Key) -> Fallible<Delta> {
        for store in self.stores.iter() {
            let result = store.get_delta(key);
            if let Ok(delta) = result {
                return Ok(delta);
            }
        }

        Err(KeyError::new(format_err!("No Key {:?} in the MultiplexDeltaStore", key)).into())
    }

    fn get_delta_chain(&self, key: &Key) -> Fallible<Vec<Delta>> {
        for store in self.stores.iter() {
            let result = store.get_delta_chain(key);
            if let Ok(delta_chain) = result {
                return Ok(delta_chain);
            }
        }

        Err(KeyError::new(format_err!("No Key {:?} in the MultiplexDeltaStore", key)).into())
    }

    fn get_meta(&self, key: &Key) -> Fallible<Metadata> {
        for store in self.stores.iter() {
            let result = store.get_meta(key);
            if let Ok(meta) = result {
                return Ok(meta);
            }
        }

        Err(KeyError::new(format_err!("No Key {:?} in the MultiplexDeltaStore", key)).into())
    }
}

impl<'a> LocalStore for MultiplexDeltaStore<'a> {
    fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>> {
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
    fn add(&self, key: &Key, info: &NodeInfo) -> Fallible<()> {
        for store in self.stores.iter() {
            store.add(key, info)?;
        }

        Ok(())
    }

    fn flush(&self) -> Fallible<Option<PathBuf>> {
        for store in self.stores.iter() {
            store.flush()?;
        }

        Ok(None)
    }
}

impl<'a> HistoryStore for MultiplexHistoryStore<'a> {
    fn get_ancestors(&self, key: &Key) -> Fallible<Ancestors> {
        for store in self.stores.iter() {
            let result = store.get_ancestors(key);
            if let Ok(ancestors) = result {
                return Ok(ancestors);
            }
        }

        Err(KeyError::new(format_err!("No Key {:?} in the MultiplexHistoryStore", key)).into())
    }

    fn get_node_info(&self, key: &Key) -> Fallible<NodeInfo> {
        for store in self.stores.iter() {
            let result = store.get_node_info(key);
            if let Ok(ancestors) = result {
                return Ok(ancestors);
            }
        }

        Err(KeyError::new(format_err!("No Key {:?} in the MultiplexHistoryStore", key)).into())
    }
}

impl<'a> LocalStore for MultiplexHistoryStore<'a> {
    fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>> {
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
    fn test_delta_add_static() -> Fallible<()> {
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
        assert_eq!(delta, read_delta);
        log.flush()?;
        Ok(())
    }

    #[test]
    fn test_delta_add_dynamic() -> Fallible<()> {
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
        assert_eq!(delta, read_delta);

        let read_delta = pack.get_delta(&delta.key)?;
        assert_eq!(delta, read_delta);

        log.flush()?;
        pack.flush()?;
        Ok(())
    }

    #[test]
    fn test_history_add_static() -> Fallible<()> {
        let tempdir = TempDir::new()?;
        let mut pack = MutableHistoryPack::new(&tempdir, HistoryPackVersion::One)?;
        let mut multiplex = MultiplexHistoryStore::new();
        multiplex.add_store(Box::new(&mut pack));

        let k = key("a", "1");
        let nodeinfo = NodeInfo {
            parents: [key("a", "2"), key("a", "3")],
            linknode: node("4"),
        };

        multiplex.add(&k, &nodeinfo)?;
        drop(multiplex);

        let read_node = pack.get_node_info(&k)?;
        assert_eq!(nodeinfo, read_node);

        pack.flush()?;
        Ok(())
    }

    #[test]
    fn test_history_add_dynamic() -> Fallible<()> {
        let tempdir = TempDir::new()?;
        let mut pack1 = MutableHistoryPack::new(&tempdir, HistoryPackVersion::One)?;
        let mut pack2 = MutableHistoryPack::new(&tempdir, HistoryPackVersion::One)?;
        let mut multiplex = MultiplexHistoryStore::new();
        multiplex.add_store(Box::new(&mut pack1));
        multiplex.add_store(Box::new(&mut pack2));

        let k = key("a", "1");
        let nodeinfo = NodeInfo {
            parents: [key("a", "2"), key("a", "3")],
            linknode: node("4"),
        };

        multiplex.add(&k, &nodeinfo)?;
        drop(multiplex);

        let read_node = pack1.get_node_info(&k)?;
        assert_eq!(nodeinfo, read_node);

        let read_node = pack2.get_node_info(&k)?;
        assert_eq!(nodeinfo, read_node);

        pack1.flush()?;
        pack2.flush()?;
        Ok(())
    }
}
