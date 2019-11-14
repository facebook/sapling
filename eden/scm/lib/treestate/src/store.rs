/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Trait defining an append-only storage system.

use crate::errors::ErrorKind;
use failure::{bail, Fallible};
use std::borrow::Cow;

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct BlockId(pub u64);

/// Append-only storage.  Blocks of data may be stored in an instance of a Store.  Once written,
/// blocks are immutable.
pub trait Store {
    /// Append a new block of data to the store.  Returns the ID of the block.  Note that blocks
    /// may be buffered until `flush` is called.
    fn append(&mut self, data: &[u8]) -> Fallible<BlockId>;

    /// Flush all appended blocks to the backing store.
    fn flush(&mut self) -> Fallible<()>;
}

/// Read-only view of a store.
pub trait StoreView {
    /// Read a block of data from the store.  Blocks are immutiable, so the result may be a
    /// reference to the internal copy of the data in the store.
    fn read<'a>(&'a self, id: BlockId) -> Fallible<Cow<'a, [u8]>>;
}

/// Null implementation of a store.  This cannot be used to store new blocks of data, and returns
/// an error if any attempts to read are made.
pub struct NullStore;

impl NullStore {
    pub fn new() -> NullStore {
        NullStore
    }
}

impl Store for NullStore {
    fn append(&mut self, _: &[u8]) -> Fallible<BlockId> {
        panic!("append to NullStore");
    }

    fn flush(&mut self) -> Fallible<()> {
        Ok(())
    }
}

impl StoreView for NullStore {
    fn read<'a>(&'a self, id: BlockId) -> Fallible<Cow<'a, [u8]>> {
        bail!(ErrorKind::InvalidStoreId(id.0))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::store::{BlockId, Store, StoreView};
    use std::borrow::Cow;
    use std::collections::HashMap;

    /// Define a Store to be used in tests.  This doesn't store the data on disk, but rather
    /// keeps it in memory in a hash map.
    pub struct MapStore {
        next_id: BlockId,
        data: HashMap<BlockId, Vec<u8>>,
    }

    impl MapStore {
        pub fn new() -> MapStore {
            // Initial ID is set to 24 to simulate a header.
            MapStore {
                next_id: BlockId(24),
                data: HashMap::new(),
            }
        }
    }

    impl Store for MapStore {
        fn append(&mut self, data: &[u8]) -> Fallible<BlockId> {
            let id = self.next_id;
            self.data.insert(id, data.to_vec());
            self.next_id.0 += data.len() as u64;
            Ok(id)
        }

        fn flush(&mut self) -> Fallible<()> {
            Ok(())
        }
    }

    impl StoreView for MapStore {
        fn read<'a>(&'a self, id: BlockId) -> Fallible<Cow<'a, [u8]>> {
            match self.data.get(&id) {
                Some(data) => Ok(Cow::from(data.as_slice())),
                None => bail!(ErrorKind::InvalidStoreId(id.0)),
            }
        }
    }

    #[test]
    fn basic_test() {
        let mut ms = MapStore::new();
        let key1 = ms.append("12345".as_bytes()).expect("append key1");
        let key2 = ms.append("67890".as_bytes()).expect("append key2");
        ms.flush().expect("flush");
        assert_eq!(ms.read(key2).unwrap(), "67890".as_bytes());
        assert_eq!(ms.read(key1).unwrap(), "12345".as_bytes());
        assert_eq!(
            ms.read(BlockId(999)).unwrap_err().to_string(),
            "invalid store id: 999"
        );
    }
}
