// Copyright Facebook, Inc. 2018
// Union history store
use std::rc::Rc;

use ancestors::BatchedAncestorIterator;
use error::{KeyError, Result};
use historystore::{Ancestors, HistoryStore, NodeInfo};
use key::Key;
use unionstore::UnionStore;

pub type UnionHistoryStore = UnionStore<Rc<HistoryStore>>;

#[derive(Debug, Fail)]
#[fail(display = "Union History Store Error: {:?}", _0)]
struct UnionHistoryStoreError(String);

impl From<UnionHistoryStoreError> for KeyError {
    fn from(err: UnionHistoryStoreError) -> Self {
        KeyError::new(err.into())
    }
}

impl UnionHistoryStore {
    fn get_partial_ancestors(&self, key: &Key) -> Result<Ancestors> {
        for store in self {
            match store.get_ancestors(key) {
                Ok(res) => return Ok(res),
                Err(e) => match e.downcast_ref::<KeyError>() {
                    Some(_) => continue,
                    None => return Err(e),
                },
            }
        }

        Err(KeyError::from(UnionHistoryStoreError(format!(
            "No ancestors found for key {:?}",
            key
        ))).into())
    }
}

impl HistoryStore for UnionHistoryStore {
    fn get_ancestors(&self, key: &Key) -> Result<Ancestors> {
        BatchedAncestorIterator::new(key, |k, _seen| self.get_partial_ancestors(k)).collect()
    }

    fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>> {
        let initial_keys = Ok(keys.iter().cloned().collect());
        self.into_iter()
            .fold(initial_keys, |missing_keys, store| match missing_keys {
                Ok(missing_keys) => store.get_missing(&missing_keys),
                Err(e) => Err(e),
            })
    }

    fn get_node_info(&self, key: &Key) -> Result<NodeInfo> {
        for store in self {
            match store.get_node_info(key) {
                Ok(res) => return Ok(res),
                Err(e) => match e.downcast_ref::<KeyError>() {
                    Some(_) => continue,
                    None => return Err(e),
                },
            }
        }

        Err(KeyError::from(UnionHistoryStoreError(format!(
            "No NodeInfo found for key {:?}",
            key
        ))).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct BadHistoryStore;

    #[derive(Debug, Fail)]
    #[fail(display = "Bad history store always has error which is not KeyError")]
    struct BadHistoryStoreError;

    struct EmptyHistoryStore;

    #[derive(Debug, Fail)]
    #[fail(display = "History Store is empty")]
    struct EmptyHistoryStoreError;

    impl From<EmptyHistoryStoreError> for KeyError {
        fn from(err: EmptyHistoryStoreError) -> Self {
            KeyError::new(err.into())
        }
    }

    impl HistoryStore for EmptyHistoryStore {
        fn get_ancestors(&self, _key: &Key) -> Result<Ancestors> {
            Err(KeyError::from(EmptyHistoryStoreError).into())
        }

        fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>> {
            Ok(keys.iter().cloned().collect())
        }

        fn get_node_info(&self, _key: &Key) -> Result<NodeInfo> {
            Err(KeyError::from(EmptyHistoryStoreError).into())
        }
    }

    impl HistoryStore for BadHistoryStore {
        fn get_ancestors(&self, _key: &Key) -> Result<Ancestors> {
            Err(BadHistoryStoreError.into())
        }

        fn get_missing(&self, _keys: &[Key]) -> Result<Vec<Key>> {
            Err(BadHistoryStoreError.into())
        }

        fn get_node_info(&self, _key: &Key) -> Result<NodeInfo> {
            Err(BadHistoryStoreError.into())
        }
    }

    quickcheck! {
        fn test_empty_unionstore_get_ancestors(key: Key) -> bool {
            match UnionHistoryStore::new().get_ancestors(&key) {
                Ok(_) => false,
                Err(e) => e.downcast_ref::<KeyError>().is_some(),
            }
        }

        fn test_empty_historystore_get_ancestors(key: Key) -> bool {
            let mut unionstore = UnionHistoryStore::new();
            unionstore.add(Rc::new(EmptyHistoryStore));
            match unionstore.get_ancestors(&key) {
                Ok(_) => false,
                Err(e) => e.downcast_ref::<KeyError>().is_some(),
            }
        }

        fn test_bad_historystore_get_ancestors(key: Key) -> bool {
            let mut unionstore = UnionHistoryStore::new();
            unionstore.add(Rc::new(BadHistoryStore));
            match unionstore.get_ancestors(&key) {
                Ok(_) => false,
                Err(e) => e.downcast_ref::<KeyError>().is_none(),
            }
        }

        fn test_empty_unionstore_get_node_info(key: Key) -> bool {
            match UnionHistoryStore::new().get_node_info(&key) {
                Ok(_) => false,
                Err(e) => e.downcast_ref::<KeyError>().is_some(),
            }
        }

        fn test_empty_historystore_get_node_info(key: Key) -> bool {
            let mut unionstore = UnionHistoryStore::new();
            unionstore.add(Rc::new(EmptyHistoryStore));
            match unionstore.get_node_info(&key) {
                Ok(_) => false,
                Err(e) => e.downcast_ref::<KeyError>().is_some(),
            }
        }

        fn test_bad_historystore_get_node_info(key: Key) -> bool {
            let mut unionstore = UnionHistoryStore::new();
            unionstore.add(Rc::new(BadHistoryStore));
            match unionstore.get_node_info(&key) {
                Ok(_) => false,
                Err(e) => e.downcast_ref::<KeyError>().is_none(),
            }
        }

        fn test_empty_unionstore_get_missing(keys: Vec<Key>) -> bool {
            keys == UnionHistoryStore::new().get_missing(&keys).unwrap()
        }

        fn test_empty_historystore_get_missing(keys: Vec<Key>) -> bool {
            let mut unionstore = UnionHistoryStore::new();
            unionstore.add(Rc::new(EmptyHistoryStore));
            keys == unionstore.get_missing(&keys).unwrap()
        }

        fn test_bad_historystore_get_missing(keys: Vec<Key>) -> bool {
            let mut unionstore = UnionHistoryStore::new();
            unionstore.add(Rc::new(BadHistoryStore));
            match unionstore.get_missing(&keys) {
                Ok(_) => false,
                Err(e) => e.downcast_ref::<KeyError>().is_none(),
            }
        }
    }
}
