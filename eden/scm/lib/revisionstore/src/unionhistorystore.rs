/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// Union history store
use anyhow::Result;
use types::Key;
use types::NodeInfo;

use crate::historystore::HgIdHistoryStore;
use crate::historystore::RemoteHistoryStore;
use crate::types::StoreKey;
use crate::unionstore::UnionStore;

pub type UnionHgIdHistoryStore<T> = UnionStore<T>;

impl<T: HgIdHistoryStore> HgIdHistoryStore for UnionHgIdHistoryStore<T> {
    fn get_node_info(&self, key: &Key) -> Result<Option<NodeInfo>> {
        for store in self {
            match store.get_node_info(key)? {
                None => continue,
                Some(res) => return Ok(Some(res)),
            }
        }

        Ok(None)
    }

    fn refresh(&self) -> Result<()> {
        for store in self {
            store.refresh()?;
        }
        Ok(())
    }
}

impl<T: RemoteHistoryStore> RemoteHistoryStore for UnionHgIdHistoryStore<T> {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<()> {
        let initial_keys = Ok(keys.to_vec());
        self.into_iter()
            .fold(initial_keys, |missing_keys, store| match missing_keys {
                Ok(missing_keys) => {
                    if !missing_keys.is_empty() {
                        store.prefetch(&missing_keys)?;
                        store.get_missing(&missing_keys)
                    } else {
                        Ok(vec![])
                    }
                }
                Err(e) => Err(e),
            })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use quickcheck::quickcheck;
    use thiserror::Error;

    use super::*;
    use crate::localstore::LocalStore;
    use crate::types::StoreKey;

    struct BadHgIdHistoryStore;

    struct EmptyHgIdHistoryStore;

    #[derive(Debug, Error)]
    #[error("Bad history store always has error which is not KeyError")]
    struct BadHgIdHistoryStoreError;

    impl HgIdHistoryStore for EmptyHgIdHistoryStore {
        fn get_node_info(&self, _key: &Key) -> Result<Option<NodeInfo>> {
            Ok(None)
        }

        fn refresh(&self) -> Result<()> {
            Ok(())
        }
    }

    impl LocalStore for EmptyHgIdHistoryStore {
        fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
            Ok(keys.iter().cloned().collect())
        }
    }

    impl HgIdHistoryStore for BadHgIdHistoryStore {
        fn get_node_info(&self, _key: &Key) -> Result<Option<NodeInfo>> {
            Err(BadHgIdHistoryStoreError.into())
        }

        fn refresh(&self) -> Result<()> {
            Ok(())
        }
    }

    impl LocalStore for BadHgIdHistoryStore {
        fn get_missing(&self, _keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
            Err(BadHgIdHistoryStoreError.into())
        }
    }

    quickcheck! {
        fn test_empty_unionstore_get_node_info(key: Key) -> bool {
            match UnionHgIdHistoryStore::<EmptyHgIdHistoryStore>::new().get_node_info(&key) {
                Ok(None) => true,
                _ => false,
            }
        }

        fn test_empty_historystore_get_node_info(key: Key) -> bool {
            let mut unionstore = UnionHgIdHistoryStore::new();
            unionstore.add(EmptyHgIdHistoryStore);
            match unionstore.get_node_info(&key) {
                Ok(None) => true,
                _ => false,
            }
        }

        fn test_bad_historystore_get_node_info(key: Key) -> bool {
            let mut unionstore = UnionHgIdHistoryStore::new();
            unionstore.add(BadHgIdHistoryStore);
            match unionstore.get_node_info(&key) {
                Err(_) => true,
                _ => false,
            }
        }

        fn test_empty_unionstore_get_missing(keys: Vec<StoreKey>) -> bool {
            keys == UnionHgIdHistoryStore::<EmptyHgIdHistoryStore>::new().get_missing(&keys).unwrap()
        }

        fn test_empty_historystore_get_missing(keys: Vec<StoreKey>) -> bool {
            let mut unionstore = UnionHgIdHistoryStore::new();
            unionstore.add(EmptyHgIdHistoryStore);
            keys == unionstore.get_missing(&keys).unwrap()
        }

        fn test_bad_historystore_get_missing(keys: Vec<StoreKey>) -> bool {
            let mut unionstore = UnionHgIdHistoryStore::new();
            unionstore.add(BadHgIdHistoryStore);
            match unionstore.get_missing(&keys) {
                Err(_) => true,
                _ => false,
            }
        }
    }
}
