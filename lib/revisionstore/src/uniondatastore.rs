// Copyright Facebook, Inc. 2018
// Union data store
use failure::{err_msg, Fail, Fallible};

use mpatch::mpatch::get_full_text;

use types::Key;

use crate::datastore::{DataStore, Delta, Metadata};
use crate::error::KeyError;
use crate::unionstore::UnionStore;

pub type UnionDataStore<T> = UnionStore<T>;

#[derive(Debug, Fail)]
#[fail(display = "Union Data Store Error: {:?}", _0)]
struct UnionDataStoreError(String);

impl From<UnionDataStoreError> for KeyError {
    fn from(err: UnionDataStoreError) -> Self {
        KeyError::new(err.into())
    }
}

impl<T: DataStore> UnionDataStore<T> {
    fn get_partial_chain(&self, key: &Key) -> Fallible<Vec<Delta>> {
        for store in self {
            match store.get_delta_chain(key) {
                Ok(res) => return Ok(res),
                Err(e) => match e.downcast_ref::<KeyError>() {
                    Some(_) => continue,
                    None => return Err(e),
                },
            }
        }

        Err(KeyError::from(UnionDataStoreError(format!(
            "No delta chain found for key {:?}",
            key
        )))
        .into())
    }
}

impl<T: DataStore> DataStore for UnionDataStore<T> {
    fn get(&self, key: &Key) -> Fallible<Vec<u8>> {
        let delta_chain = self.get_delta_chain(key)?;
        let (basetext, deltas) =
            delta_chain
                .split_last()
                .ok_or(KeyError::from(UnionDataStoreError(format!(
                    "No delta chain for key {:?}",
                    key
                ))))?;

        let deltas: Vec<&[u8]> = deltas
            .iter()
            .rev()
            .map(|delta| delta.data.as_ref())
            .collect();

        get_full_text(basetext.data.as_ref(), &deltas).map_err(|e| err_msg(e))
    }

    fn get_delta(&self, key: &Key) -> Fallible<Delta> {
        for store in self {
            match store.get_delta(key) {
                Ok(res) => return Ok(res),
                Err(e) => match e.downcast_ref::<KeyError>() {
                    Some(_) => continue,
                    None => return Err(e),
                },
            }
        }

        Err(KeyError::from(UnionDataStoreError(format!(
            "No delta found for key {:?}",
            key
        )))
        .into())
    }

    fn get_delta_chain(&self, key: &Key) -> Fallible<Vec<Delta>> {
        let mut current_key = Some(key.clone());
        let mut delta_chain = Vec::new();
        while let Some(key) = current_key {
            let partial_chain = self.get_partial_chain(&key)?;
            current_key = partial_chain
                .last()
                .ok_or(KeyError::from(UnionDataStoreError(format!(
                    "No delta chain for key {:?}",
                    key
                ))))?
                .base
                .clone();
            delta_chain.extend(partial_chain);
        }

        Ok(delta_chain)
    }

    fn get_meta(&self, key: &Key) -> Fallible<Metadata> {
        for store in self {
            match store.get_meta(key) {
                Ok(res) => return Ok(res),
                Err(e) => match e.downcast_ref::<KeyError>() {
                    Some(_) => continue,
                    None => return Err(e),
                },
            }
        }

        Err(KeyError::from(UnionDataStoreError(format!(
            "No metadata found for key {:?}",
            key
        )))
        .into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use quickcheck::quickcheck;

    use crate::store::Store;

    struct BadDataStore;

    #[derive(Debug, Fail)]
    #[fail(display = "Bad data store always has error which is not KeyError")]
    struct BadDataStoreError;

    struct EmptyDataStore;

    #[derive(Debug, Fail)]
    #[fail(display = "Data Store is empty")]
    struct EmptyDataStoreError;

    impl From<EmptyDataStoreError> for KeyError {
        fn from(err: EmptyDataStoreError) -> Self {
            KeyError::new(err.into())
        }
    }

    impl DataStore for EmptyDataStore {
        fn get(&self, _key: &Key) -> Fallible<Vec<u8>> {
            Err(KeyError::from(EmptyDataStoreError).into())
        }

        fn get_delta(&self, _key: &Key) -> Fallible<Delta> {
            Err(KeyError::from(EmptyDataStoreError).into())
        }

        fn get_delta_chain(&self, _key: &Key) -> Fallible<Vec<Delta>> {
            Err(KeyError::from(EmptyDataStoreError).into())
        }

        fn get_meta(&self, _key: &Key) -> Fallible<Metadata> {
            Err(KeyError::from(EmptyDataStoreError).into())
        }
    }

    impl Store for EmptyDataStore {
        fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>> {
            Ok(keys.iter().cloned().collect())
        }
    }

    impl DataStore for BadDataStore {
        fn get(&self, _key: &Key) -> Fallible<Vec<u8>> {
            Err(BadDataStoreError.into())
        }

        fn get_delta(&self, _key: &Key) -> Fallible<Delta> {
            Err(BadDataStoreError.into())
        }

        fn get_delta_chain(&self, _key: &Key) -> Fallible<Vec<Delta>> {
            Err(BadDataStoreError.into())
        }

        fn get_meta(&self, _key: &Key) -> Fallible<Metadata> {
            Err(BadDataStoreError.into())
        }
    }

    impl Store for BadDataStore {
        fn get_missing(&self, _keys: &[Key]) -> Fallible<Vec<Key>> {
            Err(BadDataStoreError.into())
        }
    }

    quickcheck! {
        fn test_empty_unionstore_get(key: Key) -> bool {
            match UnionDataStore::<EmptyDataStore>::new().get(&key) {
                Ok(_) => false,
                Err(e) => e.downcast_ref::<KeyError>().is_some(),
            }
        }

        fn test_empty_datastore_get(key: Key) -> bool {
            let mut unionstore = UnionDataStore::new();
            unionstore.add(EmptyDataStore);
            match unionstore.get(&key) {
                Ok(_) => false,
                Err(e) => e.downcast_ref::<KeyError>().is_some(),
            }
        }

        fn test_bad_datastore_get(key: Key) -> bool {
            let mut unionstore = UnionDataStore::new();
            unionstore.add(BadDataStore);
            match unionstore.get(&key) {
                Ok(_) => false,
                Err(e) => e.downcast_ref::<KeyError>().is_none(),
            }
        }

        fn test_empty_unionstore_get_delta_chain(key: Key) -> bool {
            match UnionDataStore::<EmptyDataStore>::new().get_delta_chain(&key) {
                Ok(_) => false,
                Err(e) => e.downcast_ref::<KeyError>().is_some(),
            }
        }

        fn test_empty_datastore_get_delta_chain(key: Key) -> bool {
            let mut unionstore = UnionDataStore::new();
            unionstore.add(EmptyDataStore);
            match unionstore.get_delta_chain(&key) {
                Ok(_) => false,
                Err(e) => e.downcast_ref::<KeyError>().is_some(),
            }
        }

        fn test_bad_datastore_get_delta_chain(key: Key) -> bool {
            let mut unionstore = UnionDataStore::new();
            unionstore.add(BadDataStore);
            match unionstore.get_delta_chain(&key) {
                Ok(_) => false,
                Err(e) => e.downcast_ref::<KeyError>().is_none(),
            }
        }

        fn test_empty_unionstore_get_meta(key: Key) -> bool {
            match UnionDataStore::<EmptyDataStore>::new().get_meta(&key) {
                Ok(_) => false,
                Err(e) => e.downcast_ref::<KeyError>().is_some(),
            }
        }

        fn test_empty_datastore_get_meta(key: Key) -> bool {
            let mut unionstore = UnionDataStore::new();
            unionstore.add(EmptyDataStore);
            match unionstore.get_meta(&key) {
                Ok(_) => false,
                Err(e) => e.downcast_ref::<KeyError>().is_some(),
            }
        }

        fn test_bad_datastore_get_meta(key: Key) -> bool {
            let mut unionstore = UnionDataStore::new();
            unionstore.add(BadDataStore);
            match unionstore.get_meta(&key) {
                Ok(_) => false,
                Err(e) => e.downcast_ref::<KeyError>().is_none(),
            }
        }

        fn test_empty_unionstore_get_missing(keys: Vec<Key>) -> bool {
            keys == UnionDataStore::<EmptyDataStore>::new().get_missing(&keys).unwrap()
        }

        fn test_empty_datastore_get_missing(keys: Vec<Key>) -> bool {
            let mut unionstore = UnionDataStore::new();
            unionstore.add(EmptyDataStore);
            keys == unionstore.get_missing(&keys).unwrap()
        }

        fn test_bad_datastore_get_missing(keys: Vec<Key>) -> bool {
            let mut unionstore = UnionDataStore::new();
            unionstore.add(BadDataStore);
            match unionstore.get_missing(&keys) {
                Ok(_) => false,
                Err(e) => e.downcast_ref::<KeyError>().is_none(),
            }
        }
    }
}
