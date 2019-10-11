// Copyright Facebook, Inc. 2018
// Union history store
use failure::Fallible;

use types::{Key, NodeInfo};

use crate::ancestors::{AncestorTraversal, BatchedAncestorIterator};
use crate::historystore::{Ancestors, HistoryStore};
use crate::unionstore::UnionStore;

pub type UnionHistoryStore<T> = UnionStore<T>;

impl<T: HistoryStore> UnionHistoryStore<T> {
    fn get_partial_ancestors(&self, key: &Key) -> Fallible<Option<Ancestors>> {
        for store in self {
            match store.get_ancestors(key)? {
                None => continue,
                Some(res) => return Ok(Some(res)),
            }
        }

        Ok(None)
    }
}

impl<T: HistoryStore> HistoryStore for UnionHistoryStore<T> {
    fn get_ancestors(&self, key: &Key) -> Fallible<Option<Ancestors>> {
        BatchedAncestorIterator::new(
            key,
            |k, _seen| self.get_partial_ancestors(k),
            AncestorTraversal::Complete,
        )
        .collect()
    }

    fn get_node_info(&self, key: &Key) -> Fallible<Option<NodeInfo>> {
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
        fn get_ancestors(&self, _key: &Key) -> Fallible<Option<Ancestors>> {
            Ok(None)
        }

        fn get_node_info(&self, _key: &Key) -> Fallible<Option<NodeInfo>> {
            Ok(None)
        }
    }

    impl LocalStore for EmptyHistoryStore {
        fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>> {
            Ok(keys.iter().cloned().collect())
        }
    }

    impl HistoryStore for BadHistoryStore {
        fn get_ancestors(&self, _key: &Key) -> Fallible<Option<Ancestors>> {
            Err(BadHistoryStoreError.into())
        }

        fn get_node_info(&self, _key: &Key) -> Fallible<Option<NodeInfo>> {
            Err(BadHistoryStoreError.into())
        }
    }

    impl LocalStore for BadHistoryStore {
        fn get_missing(&self, _keys: &[Key]) -> Fallible<Vec<Key>> {
            Err(BadHistoryStoreError.into())
        }
    }

    quickcheck! {
        fn test_empty_unionstore_get_ancestors(key: Key) -> bool {
            match UnionHistoryStore::<EmptyHistoryStore>::new().get_ancestors(&key) {
                Ok(None) => true,
                _ => false,
            }
        }

        fn test_empty_historystore_get_ancestors(key: Key) -> bool {
            let mut unionstore = UnionHistoryStore::new();
            unionstore.add(EmptyHistoryStore);
            match unionstore.get_ancestors(&key) {
                Ok(None) => true,
                _ => false,
            }
        }

        fn test_bad_historystore_get_ancestors(key: Key) -> bool {
            let mut unionstore = UnionHistoryStore::new();
            unionstore.add(BadHistoryStore);
            match unionstore.get_ancestors(&key) {
                Err(_) => true,
                _ => false,
            }
        }

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
