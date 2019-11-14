/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// Union history store
use failure::Fallible as Result;

use types::{Key, NodeInfo};

use crate::historystore::HistoryStore;
use crate::unionstore::UnionStore;

pub type UnionHistoryStore<T> = UnionStore<T>;

impl<T: HistoryStore> HistoryStore for UnionHistoryStore<T> {
    fn get_node_info(&self, key: &Key) -> Result<Option<NodeInfo>> {
        for store in self {
            match store.get_node_info(key)? {
                None => continue,
                Some(res) => return Ok(Some(res)),
            }
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use failure::Fail;
    use quickcheck::quickcheck;

    use crate::localstore::LocalStore;

    struct BadHistoryStore;

    struct EmptyHistoryStore;

    #[derive(Debug, Fail)]
    #[fail(display = "Bad history store always has error which is not KeyError")]
    struct BadHistoryStoreError;

    impl HistoryStore for EmptyHistoryStore {
        fn get_node_info(&self, _key: &Key) -> Result<Option<NodeInfo>> {
            Ok(None)
        }
    }

    impl LocalStore for EmptyHistoryStore {
        fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>> {
            Ok(keys.iter().cloned().collect())
        }
    }

    impl HistoryStore for BadHistoryStore {
        fn get_node_info(&self, _key: &Key) -> Result<Option<NodeInfo>> {
            Err(BadHistoryStoreError.into())
        }
    }

    impl LocalStore for BadHistoryStore {
        fn get_missing(&self, _keys: &[Key]) -> Result<Vec<Key>> {
            Err(BadHistoryStoreError.into())
        }
    }

    quickcheck! {
        fn test_empty_unionstore_get_node_info(key: Key) -> bool {
            match UnionHistoryStore::<EmptyHistoryStore>::new().get_node_info(&key) {
                Ok(None) => true,
                _ => false,
            }
        }

        fn test_empty_historystore_get_node_info(key: Key) -> bool {
            let mut unionstore = UnionHistoryStore::new();
            unionstore.add(EmptyHistoryStore);
            match unionstore.get_node_info(&key) {
                Ok(None) => true,
                _ => false,
            }
        }

        fn test_bad_historystore_get_node_info(key: Key) -> bool {
            let mut unionstore = UnionHistoryStore::new();
            unionstore.add(BadHistoryStore);
            match unionstore.get_node_info(&key) {
                Err(_) => true,
                _ => false,
            }
        }

        fn test_empty_unionstore_get_missing(keys: Vec<Key>) -> bool {
            keys == UnionHistoryStore::<EmptyHistoryStore>::new().get_missing(&keys).unwrap()
        }

        fn test_empty_historystore_get_missing(keys: Vec<Key>) -> bool {
            let mut unionstore = UnionHistoryStore::new();
            unionstore.add(EmptyHistoryStore);
            keys == unionstore.get_missing(&keys).unwrap()
        }

        fn test_bad_historystore_get_missing(keys: Vec<Key>) -> bool {
            let mut unionstore = UnionHistoryStore::new();
            unionstore.add(BadHistoryStore);
            match unionstore.get_missing(&keys) {
                Err(_) => true,
                _ => false,
            }
        }
    }
}
