/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// Union data store
use anyhow::Result;

use bytes::Bytes;

use types::Key;

use crate::{
    datastore::{ContentDataStore, ContentMetadata, HgIdDataStore, Metadata, RemoteDataStore},
    types::StoreKey,
    unionstore::UnionStore,
};

pub type UnionHgIdDataStore<T> = UnionStore<T>;

impl<T: HgIdDataStore> HgIdDataStore for UnionHgIdDataStore<T> {
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>> {
        for store in self {
            if let Some(result) = store.get(key)? {
                return Ok(Some(result));
            }
        }

        Ok(None)
    }

    fn get_meta(&self, key: &Key) -> Result<Option<Metadata>> {
        for store in self {
            if let Some(meta) = store.get_meta(key)? {
                return Ok(Some(meta));
            }
        }

        Ok(None)
    }
}

impl<T: RemoteDataStore> RemoteDataStore for UnionHgIdDataStore<T> {
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

    fn upload(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.into_iter()
            .fold(Ok(keys.to_vec()), |not_sent, store| match not_sent {
                Ok(not_sent) => {
                    if !not_sent.is_empty() {
                        store.upload(&not_sent)
                    } else {
                        Ok(Vec::new())
                    }
                }
                Err(e) => Err(e),
            })
    }
}

pub type UnionContentDataStore<T> = UnionStore<T>;

impl<T: ContentDataStore> ContentDataStore for UnionContentDataStore<T> {
    fn blob(&self, key: &StoreKey) -> Result<Option<Bytes>> {
        for store in self {
            if let Some(data) = store.blob(key)? {
                return Ok(Some(data));
            }
        }

        Ok(None)
    }

    fn metadata(&self, key: &StoreKey) -> Result<Option<ContentMetadata>> {
        for store in self {
            if let Some(meta) = store.metadata(key)? {
                return Ok(Some(meta));
            }
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use quickcheck::quickcheck;
    use thiserror::Error;

    use crate::{localstore::LocalStore, types::StoreKey};

    struct BadHgIdDataStore;

    #[derive(Debug, Error)]
    #[error("Bad data store always has error which is not KeyError")]
    struct BadHgIdDataStoreError;

    struct EmptyHgIdDataStore;

    impl HgIdDataStore for EmptyHgIdDataStore {
        fn get(&self, _key: &Key) -> Result<Option<Vec<u8>>> {
            Ok(None)
        }

        fn get_meta(&self, _key: &Key) -> Result<Option<Metadata>> {
            Ok(None)
        }
    }

    impl LocalStore for EmptyHgIdDataStore {
        fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
            Ok(keys.iter().cloned().collect())
        }
    }

    impl HgIdDataStore for BadHgIdDataStore {
        fn get(&self, _key: &Key) -> Result<Option<Vec<u8>>> {
            Err(BadHgIdDataStoreError.into())
        }

        fn get_meta(&self, _key: &Key) -> Result<Option<Metadata>> {
            Err(BadHgIdDataStoreError.into())
        }
    }

    impl LocalStore for BadHgIdDataStore {
        fn get_missing(&self, _keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
            Err(BadHgIdDataStoreError.into())
        }
    }

    quickcheck! {
        fn test_empty_unionstore_get(key: Key) -> bool {
            match UnionHgIdDataStore::<EmptyHgIdDataStore>::new().get(&key) {
                Ok(None) => true,
                _ => false,
            }
        }

        fn test_empty_datastore_get(key: Key) -> bool {
            let mut unionstore = UnionHgIdDataStore::new();
            unionstore.add(EmptyHgIdDataStore);
            match unionstore.get(&key) {
                Ok(None) => true,
                _ => false,
            }
        }

        fn test_bad_datastore_get(key: Key) -> bool {
            let mut unionstore = UnionHgIdDataStore::new();
            unionstore.add(BadHgIdDataStore);
            match unionstore.get(&key) {
                Err(_) => true,
                _ => false,
            }
        }

        fn test_empty_unionstore_get_meta(key: Key) -> bool {
            match UnionHgIdDataStore::<EmptyHgIdDataStore>::new().get_meta(&key) {
                Ok(None) => true,
                _ => false,
            }
        }

        fn test_empty_datastore_get_meta(key: Key) -> bool {
            let mut unionstore = UnionHgIdDataStore::new();
            unionstore.add(EmptyHgIdDataStore);
            match unionstore.get_meta(&key) {
                Ok(None) => true,
                _ => false,
            }
        }

        fn test_bad_datastore_get_meta(key: Key) -> bool {
            let mut unionstore = UnionHgIdDataStore::new();
            unionstore.add(BadHgIdDataStore);
            match unionstore.get_meta(&key) {
                Err(_) => true,
                _ => false,
            }
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
            match unionstore.get_missing(&keys) {
                Ok(_) => false,
                Err(_) => true,
            }
        }
    }
}
