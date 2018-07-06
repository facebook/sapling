// Copyright Facebook, Inc. 2018
// Union data store
extern crate mpatch;

use std::rc::Rc;

use failure::err_msg;

use datastore::{DataStore, Delta, Metadata};
use error::{KeyError, Result};
use key::Key;
use unionstore::UnionStore;

use self::mpatch::mpatch::get_full_text;

pub type UnionDataStore = UnionStore<Rc<DataStore>>;

#[derive(Debug, Fail)]
#[fail(display = "Union Data Store Error: {:?}", _0)]
struct UnionDataStoreError(String);

impl From<UnionDataStoreError> for KeyError {
    fn from(err: UnionDataStoreError) -> Self {
        KeyError::new(err.into())
    }
}

impl UnionDataStore {
    fn get_partial_chain(&self, key: &Key) -> Result<Vec<Delta>> {
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
        ))).into())
    }
}

impl DataStore for UnionDataStore {
    fn get(&self, key: &Key) -> Result<Vec<u8>> {
        let delta_chain = self.get_delta_chain(key)?;
        let (basetext, deltas) = delta_chain.split_last().ok_or(KeyError::from(
            UnionDataStoreError(format!("No delta chain for key {:?}", key)),
        ))?;

        let deltas: Vec<&[u8]> = deltas
            .iter()
            .rev()
            .map(|delta| delta.data.as_ref())
            .collect();

        get_full_text(basetext.data.as_ref(), &deltas).map_err(|e| err_msg(e))
    }

    fn get_delta_chain(&self, key: &Key) -> Result<Vec<Delta>> {
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

    fn get_meta(&self, key: &Key) -> Result<Metadata> {
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
        ))).into())
    }

    fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>> {
        let initial_keys = Ok(keys.iter().cloned().collect());
        self.into_iter()
            .fold(initial_keys, |missing_keys, store| match missing_keys {
                Ok(missing_keys) => store.get_missing(&missing_keys),
                Err(e) => Err(e),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        fn get(&self, _key: &Key) -> Result<Vec<u8>> {
            Err(KeyError::from(EmptyDataStoreError).into())
        }

        fn get_delta_chain(&self, _key: &Key) -> Result<Vec<Delta>> {
            Err(KeyError::from(EmptyDataStoreError).into())
        }

        fn get_meta(&self, _key: &Key) -> Result<Metadata> {
            Err(KeyError::from(EmptyDataStoreError).into())
        }

        fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>> {
            Ok(keys.iter().cloned().collect())
        }
    }

    impl DataStore for BadDataStore {
        fn get(&self, _key: &Key) -> Result<Vec<u8>> {
            Err(BadDataStoreError.into())
        }

        fn get_delta_chain(&self, _key: &Key) -> Result<Vec<Delta>> {
            Err(BadDataStoreError.into())
        }

        fn get_meta(&self, _key: &Key) -> Result<Metadata> {
            Err(BadDataStoreError.into())
        }

        fn get_missing(&self, _keys: &[Key]) -> Result<Vec<Key>> {
            Err(BadDataStoreError.into())
        }
    }

    quickcheck! {
        fn test_empty_unionstore_get(key: Key) -> bool {
            match UnionDataStore::new().get(&key) {
                Ok(_) => false,
                Err(e) => e.downcast_ref::<KeyError>().is_some(),
            }
        }

        fn test_empty_datastore_get(key: Key) -> bool {
            let mut unionstore = UnionDataStore::new();
            unionstore.add(Rc::new(EmptyDataStore));
            match unionstore.get(&key) {
                Ok(_) => false,
                Err(e) => e.downcast_ref::<KeyError>().is_some(),
            }
        }

        fn test_bad_datastore_get(key: Key) -> bool {
            let mut unionstore = UnionDataStore::new();
            unionstore.add(Rc::new(BadDataStore));
            match unionstore.get(&key) {
                Ok(_) => false,
                Err(e) => e.downcast_ref::<KeyError>().is_none(),
            }
        }

        fn test_empty_unionstore_get_delta_chain(key: Key) -> bool {
            match UnionDataStore::new().get_delta_chain(&key) {
                Ok(_) => false,
                Err(e) => e.downcast_ref::<KeyError>().is_some(),
            }
        }

        fn test_empty_datastore_get_delta_chain(key: Key) -> bool {
            let mut unionstore = UnionDataStore::new();
            unionstore.add(Rc::new(EmptyDataStore));
            match unionstore.get_delta_chain(&key) {
                Ok(_) => false,
                Err(e) => e.downcast_ref::<KeyError>().is_some(),
            }
        }

        fn test_bad_datastore_get_delta_chain(key: Key) -> bool {
            let mut unionstore = UnionDataStore::new();
            unionstore.add(Rc::new(BadDataStore));
            match unionstore.get_delta_chain(&key) {
                Ok(_) => false,
                Err(e) => e.downcast_ref::<KeyError>().is_none(),
            }
        }

        fn test_empty_unionstore_get_meta(key: Key) -> bool {
            match UnionDataStore::new().get_meta(&key) {
                Ok(_) => false,
                Err(e) => e.downcast_ref::<KeyError>().is_some(),
            }
        }

        fn test_empty_datastore_get_meta(key: Key) -> bool {
            let mut unionstore = UnionDataStore::new();
            unionstore.add(Rc::new(EmptyDataStore));
            match unionstore.get_meta(&key) {
                Ok(_) => false,
                Err(e) => e.downcast_ref::<KeyError>().is_some(),
            }
        }

        fn test_bad_datastore_get_meta(key: Key) -> bool {
            let mut unionstore = UnionDataStore::new();
            unionstore.add(Rc::new(BadDataStore));
            match unionstore.get_meta(&key) {
                Ok(_) => false,
                Err(e) => e.downcast_ref::<KeyError>().is_none(),
            }
        }

        fn test_empty_unionstore_get_missing(keys: Vec<Key>) -> bool {
            keys == UnionDataStore::new().get_missing(&keys).unwrap()
        }

        fn test_empty_datastore_get_missing(keys: Vec<Key>) -> bool {
            let mut unionstore = UnionDataStore::new();
            unionstore.add(Rc::new(EmptyDataStore));
            keys == unionstore.get_missing(&keys).unwrap()
        }

        fn test_bad_datastore_get_missing(keys: Vec<Key>) -> bool {
            let mut unionstore = UnionDataStore::new();
            unionstore.add(Rc::new(BadDataStore));
            match unionstore.get_missing(&keys) {
                Ok(_) => false,
                Err(e) => e.downcast_ref::<KeyError>().is_none(),
            }
        }
    }
}
