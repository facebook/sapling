/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// Union data store
use anyhow::Result;

use crate::datastore::HgIdDataStore;
use crate::datastore::RemoteDataStore;
use crate::datastore::StoreResult;
use crate::types::StoreKey;
use crate::unionstore::UnionStore;

pub type UnionHgIdDataStore<T> = UnionStore<T>;

impl<T: HgIdDataStore> HgIdDataStore for UnionHgIdDataStore<T> {
    fn get(&self, mut key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        for store in self {
            match store.get(key)? {
                StoreResult::Found(data) => return Ok(StoreResult::Found(data)),
                StoreResult::NotFound(next) => key = next,
            }
        }

        Ok(StoreResult::NotFound(key))
    }

    fn refresh(&self) -> Result<()> {
        for store in self {
            store.refresh()?;
        }
        Ok(())
    }
}

impl<T: RemoteDataStore> RemoteDataStore for UnionHgIdDataStore<T> {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        let initial_keys = Ok(keys.to_vec());
        self.into_iter()
            .fold(initial_keys, |missing_keys, store| match missing_keys {
                Ok(missing_keys) => {
                    if !missing_keys.is_empty() {
                        store.prefetch(&missing_keys)
                    } else {
                        Ok(vec![])
                    }
                }
                Err(e) => Err(e),
            })
    }

    fn upload(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.into_iter().try_fold(keys.to_vec(), |not_sent, store| {
            if !not_sent.is_empty() {
                store.upload(&not_sent)
            } else {
                Ok(Vec::new())
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use quickcheck::quickcheck;
    use thiserror::Error;
    use types::Key;

    use super::*;
    use crate::localstore::LocalStore;
    use crate::types::StoreKey;

    struct BadHgIdDataStore;

    #[derive(Debug, Error)]
    #[error("Bad data store always has error which is not KeyError")]
    struct BadHgIdDataStoreError;

    struct EmptyHgIdDataStore;

    impl HgIdDataStore for EmptyHgIdDataStore {
        fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
            Ok(StoreResult::NotFound(key))
        }

        fn refresh(&self) -> Result<()> {
            Ok(())
        }
    }

    impl LocalStore for EmptyHgIdDataStore {
        fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
            Ok(keys.to_vec())
        }
    }

    impl HgIdDataStore for BadHgIdDataStore {
        fn get(&self, _key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
            Err(BadHgIdDataStoreError.into())
        }

        fn refresh(&self) -> Result<()> {
            Ok(())
        }
    }

    impl LocalStore for BadHgIdDataStore {
        fn get_missing(&self, _keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
            Err(BadHgIdDataStoreError.into())
        }
    }

    quickcheck! {
        fn test_empty_unionstore_get(key: Key) -> bool {
            match UnionHgIdDataStore::<EmptyHgIdDataStore>::new().get(StoreKey::hgid(key)) {
                Ok(StoreResult::NotFound(_)) => true,
                _ => false,
            }
        }

        fn test_empty_datastore_get(key: Key) -> bool {
            let mut unionstore = UnionHgIdDataStore::new();
            unionstore.add(EmptyHgIdDataStore);
            match unionstore.get(StoreKey::hgid(key)) {
                Ok(StoreResult::NotFound(_)) => true,
                _ => false,
            }
        }

        fn test_bad_datastore_get(key: Key) -> bool {
            let mut unionstore = UnionHgIdDataStore::new();
            unionstore.add(BadHgIdDataStore);
            unionstore.get(StoreKey::hgid(key)).is_err()
        }

        fn test_empty_unionstore_get_missing(keys: Vec<StoreKey>) -> bool {
            keys == UnionHgIdDataStore::<EmptyHgIdDataStore>::new().get_missing(&keys).unwrap()
        }

        fn test_empty_datastore_get_missing(keys: Vec<StoreKey>) -> bool {
            let mut unionstore = UnionHgIdDataStore::new();
            unionstore.add(EmptyHgIdDataStore);
            keys == unionstore.get_missing(&keys).unwrap()
        }

        fn test_bad_datastore_get_missing(keys: Vec<StoreKey>) -> bool {
            let mut unionstore = UnionHgIdDataStore::new();
            unionstore.add(BadHgIdDataStore);
            unionstore.get_missing(&keys).is_err()
        }
    }
}
