// Copyright Facebook, Inc. 2018
// Union data store
use failure::{err_msg, Fallible};

use mpatch::mpatch::get_full_text;

use types::Key;

use crate::datastore::{DataStore, Delta, Metadata};
use crate::unionstore::UnionStore;

pub type UnionDataStore<T> = UnionStore<T>;

impl<T: DataStore> UnionDataStore<T> {
    fn get_partial_chain(&self, key: &Key) -> Fallible<Option<Vec<Delta>>> {
        for store in self {
            match store.get_delta_chain(key)? {
                None => continue,
                Some(res) => return Ok(Some(res)),
            }
        }

        Ok(None)
    }
}

impl<T: DataStore> DataStore for UnionDataStore<T> {
    fn get(&self, key: &Key) -> Fallible<Option<Vec<u8>>> {
        let delta_chain = self.get_delta_chain(key)?;
        let delta_chain = match delta_chain {
            Some(chain) => chain,
            None => return Ok(None),
        };

        let (basetext, deltas) = match delta_chain.split_last() {
            Some((base, delta)) => (base, delta),
            None => return Ok(None),
        };

        let deltas: Vec<&[u8]> = deltas
            .iter()
            .rev()
            .map(|delta| delta.data.as_ref())
            .collect();

        Ok(Some(
            get_full_text(basetext.data.as_ref(), &deltas).map_err(|e| err_msg(e))?,
        ))
    }

    fn get_delta(&self, key: &Key) -> Fallible<Option<Delta>> {
        for store in self {
            if let Some(delta) = store.get_delta(key)? {
                return Ok(Some(delta));
            }
        }

        Ok(None)
    }

    fn get_delta_chain(&self, key: &Key) -> Fallible<Option<Vec<Delta>>> {
        let mut current_key = Some(key.clone());
        let mut delta_chain = Vec::new();
        while let Some(key) = current_key {
            let partial_chain = match self.get_partial_chain(&key)? {
                None => return Ok(None),
                Some(chain) => chain,
            };
            current_key = match partial_chain.last() {
                None => return Ok(None),
                Some(delta) => delta.base.clone(),
            };
            delta_chain.extend(partial_chain);
        }

        Ok(Some(delta_chain))
    }

    fn get_meta(&self, key: &Key) -> Fallible<Option<Metadata>> {
        for store in self {
            if let Some(meta) = store.get_meta(key)? {
                return Ok(Some(meta));
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

    struct BadDataStore;

    #[derive(Debug, Fail)]
    #[fail(display = "Bad data store always has error which is not KeyError")]
    struct BadDataStoreError;

    struct EmptyDataStore;

    impl DataStore for EmptyDataStore {
        fn get(&self, _key: &Key) -> Fallible<Option<Vec<u8>>> {
            Ok(None)
        }

        fn get_delta(&self, _key: &Key) -> Fallible<Option<Delta>> {
            Ok(None)
        }

        fn get_delta_chain(&self, _key: &Key) -> Fallible<Option<Vec<Delta>>> {
            Ok(None)
        }

        fn get_meta(&self, _key: &Key) -> Fallible<Option<Metadata>> {
            Ok(None)
        }
    }

    impl LocalStore for EmptyDataStore {
        fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>> {
            Ok(keys.iter().cloned().collect())
        }
    }

    impl DataStore for BadDataStore {
        fn get(&self, _key: &Key) -> Fallible<Option<Vec<u8>>> {
            Err(BadDataStoreError.into())
        }

        fn get_delta(&self, _key: &Key) -> Fallible<Option<Delta>> {
            Err(BadDataStoreError.into())
        }

        fn get_delta_chain(&self, _key: &Key) -> Fallible<Option<Vec<Delta>>> {
            Err(BadDataStoreError.into())
        }

        fn get_meta(&self, _key: &Key) -> Fallible<Option<Metadata>> {
            Err(BadDataStoreError.into())
        }
    }

    impl LocalStore for BadDataStore {
        fn get_missing(&self, _keys: &[Key]) -> Fallible<Vec<Key>> {
            Err(BadDataStoreError.into())
        }
    }

    quickcheck! {
        fn test_empty_unionstore_get(key: Key) -> bool {
            match UnionDataStore::<EmptyDataStore>::new().get(&key) {
                Ok(None) => true,
                _ => false,
            }
        }

        fn test_empty_datastore_get(key: Key) -> bool {
            let mut unionstore = UnionDataStore::new();
            unionstore.add(EmptyDataStore);
            match unionstore.get(&key) {
                Ok(None) => true,
                _ => false,
            }
        }

        fn test_bad_datastore_get(key: Key) -> bool {
            let mut unionstore = UnionDataStore::new();
            unionstore.add(BadDataStore);
            match unionstore.get(&key) {
                Err(e) => true,
                _ => false,
            }
        }

        fn test_empty_unionstore_get_delta_chain(key: Key) -> bool {
            match UnionDataStore::<EmptyDataStore>::new().get_delta_chain(&key) {
                Ok(None) => true,
                _ => false,
            }
        }

        fn test_empty_datastore_get_delta_chain(key: Key) -> bool {
            let mut unionstore = UnionDataStore::new();
            unionstore.add(EmptyDataStore);
            match unionstore.get_delta_chain(&key) {
                Ok(None) => true,
                _ => false,
            }
        }

        fn test_bad_datastore_get_delta_chain(key: Key) -> bool {
            let mut unionstore = UnionDataStore::new();
            unionstore.add(BadDataStore);
            match unionstore.get_delta_chain(&key) {
                Err(e) => true,
                _ => false,
            }
        }

        fn test_empty_unionstore_get_meta(key: Key) -> bool {
            match UnionDataStore::<EmptyDataStore>::new().get_meta(&key) {
                Ok(None) => true,
                _ => false,
            }
        }

        fn test_empty_datastore_get_meta(key: Key) -> bool {
            let mut unionstore = UnionDataStore::new();
            unionstore.add(EmptyDataStore);
            match unionstore.get_meta(&key) {
                Ok(None) => true,
                _ => false,
            }
        }

        fn test_bad_datastore_get_meta(key: Key) -> bool {
            let mut unionstore = UnionDataStore::new();
            unionstore.add(BadDataStore);
            match unionstore.get_meta(&key) {
                Err(e) => true,
                _ => false,
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
                Err(e) => true,
            }
        }
    }
}
