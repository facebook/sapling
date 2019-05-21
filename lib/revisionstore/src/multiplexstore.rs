// Copyright Facebook, Inc. 2019

use std::path::PathBuf;

use failure::Fallible;

use types::{Key, NodeInfo};

use crate::datastore::{Delta, Metadata, MutableDeltaStore};
use crate::historystore::MutableHistoryStore;

/// A `MultiplexStore` is a store that will duplicate all the writes to all the
/// stores that it is made of.
pub struct MultiplexStore<'a, T: ?Sized> {
    stores: Vec<&'a mut T>,
}

pub type MultiplexDeltaStore<'a, T> = MultiplexStore<'a, T>;
pub type MultiplexHistoryStore<'a, T> = MultiplexStore<'a, T>;

impl<'a, T: ?Sized> MultiplexStore<'a, T> {
    pub fn new() -> Self {
        Self { stores: Vec::new() }
    }

    pub fn add_store(&mut self, store: &'a mut T) {
        self.stores.push(store)
    }
}

impl<'a, T: MutableDeltaStore + ?Sized> MutableDeltaStore for MultiplexDeltaStore<'a, T> {
    /// Write the `Delta` and `Metadata` to all the stores
    fn add(&mut self, delta: &Delta, metadata: &Metadata) -> Fallible<()> {
        for store in self.stores.iter_mut() {
            store.add(delta, metadata)?;
        }

        Ok(())
    }

    fn close(self) -> Fallible<Option<PathBuf>> {
        // close() cannot be implemented as the concrete types of the stores aren't known
        // statically. For now, the user of this MultiplexDeltaStore would have to manually close
        // all of the stores.
        unimplemented!()
    }
}

impl<'a, T: MutableHistoryStore + ?Sized> MutableHistoryStore for MultiplexHistoryStore<'a, T> {
    fn add(&mut self, key: &Key, info: &NodeInfo) -> Fallible<()> {
        for store in self.stores.iter_mut() {
            store.add(key, info)?;
        }

        Ok(())
    }

    fn close(self) -> Fallible<Option<PathBuf>> {
        // close() cannot be implemented as the concrete types of the stores aren't known
        // statically. For now, the user of this MultiplexHistoryStore would have to manually close
        // all of the stores.
        unimplemented!()
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
        multiplex.add_store(&mut log);

        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: key("a", "1"),
        };
        let metadata = Default::default();

        multiplex.add(&delta, &metadata)?;
        let read_delta = log.get_delta(&delta.key)?;
        assert_eq!(delta, read_delta);
        log.close()?;
        Ok(())
    }

    #[test]
    fn test_delta_add_dynamic() -> Fallible<()> {
        let tempdir = TempDir::new()?;
        let mut log = IndexedLogDataStore::new(&tempdir)?;
        let mut pack = MutableDataPack::new(&tempdir, DataPackVersion::One)?;
        let mut multiplex: MultiplexDeltaStore<dyn MutableDeltaStore> = MultiplexDeltaStore::new();
        multiplex.add_store(&mut log);
        multiplex.add_store(&mut pack);

        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: key("a", "1"),
        };
        let metadata = Default::default();

        multiplex.add(&delta, &metadata)?;

        let read_delta = log.get_delta(&delta.key)?;
        assert_eq!(delta, read_delta);

        let read_delta = pack.get_delta(&delta.key)?;
        assert_eq!(delta, read_delta);

        log.close()?;
        pack.close()?;
        Ok(())
    }

    #[test]
    fn test_history_add_static() -> Fallible<()> {
        let tempdir = TempDir::new()?;
        let mut pack = MutableHistoryPack::new(&tempdir, HistoryPackVersion::One)?;
        let mut multiplex = MultiplexDeltaStore::new();
        multiplex.add_store(&mut pack);

        let k = key("a", "1");
        let nodeinfo = NodeInfo {
            parents: [key("a", "2"), key("a", "3")],
            linknode: node("4"),
        };

        multiplex.add(&k, &nodeinfo)?;
        let read_node = pack.get_node_info(&k)?;
        assert_eq!(nodeinfo, read_node);

        pack.close()?;
        Ok(())
    }

    #[test]
    fn test_history_add_dynamic() -> Fallible<()> {
        let tempdir = TempDir::new()?;
        let mut pack1 = MutableHistoryPack::new(&tempdir, HistoryPackVersion::One)?;
        let mut pack2 = MutableHistoryPack::new(&tempdir, HistoryPackVersion::One)?;
        let mut multiplex: MultiplexHistoryStore<dyn MutableHistoryStore> =
            MultiplexDeltaStore::new();
        multiplex.add_store(&mut pack1);
        multiplex.add_store(&mut pack2);

        let k = key("a", "1");
        let nodeinfo = NodeInfo {
            parents: [key("a", "2"), key("a", "3")],
            linknode: node("4"),
        };

        multiplex.add(&k, &nodeinfo)?;
        let read_node = pack1.get_node_info(&k)?;
        assert_eq!(nodeinfo, read_node);

        let read_node = pack2.get_node_info(&k)?;
        assert_eq!(nodeinfo, read_node);

        pack1.close()?;
        pack2.close()?;
        Ok(())
    }
}
