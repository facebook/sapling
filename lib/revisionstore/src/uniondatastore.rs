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
#[fail(display = "Union Store Error: {:?}", _0)]
struct UnionDataStoreError(String);

impl From<UnionDataStoreError> for KeyError {
    fn from(err: UnionDataStoreError) -> Self {
        KeyError::new(err.into())
    }
}

impl UnionDataStore {
    fn getpartialchain(&self, key: &Key) -> Result<Vec<Delta>> {
        for store in self {
            match store.getdeltachain(key) {
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
        let deltachain = self.getdeltachain(key)?;
        let (basetext, deltas) = deltachain.split_last().ok_or(KeyError::from(
            UnionDataStoreError(format!("No delta chain for key {:?}", key)),
        ))?;

        let deltas: Vec<&[u8]> = deltas
            .iter()
            .rev()
            .map(|delta| delta.data.as_ref())
            .collect();

        get_full_text(basetext.data.as_ref(), &deltas).map_err(|e| err_msg(e))
    }

    fn getdeltachain(&self, key: &Key) -> Result<Vec<Delta>> {
        let mut currentkey = key.clone();
        let mut deltachain = Vec::new();
        while !currentkey.node().is_null() {
            let partialchain = self.getpartialchain(&currentkey)?;
            currentkey = partialchain
                .last()
                .ok_or(KeyError::from(UnionDataStoreError(format!(
                    "No delta chain for key {:?}",
                    currentkey
                ))))?
                .base
                .clone();
            deltachain.extend(partialchain);
        }

        Ok(deltachain)
    }

    fn getmeta(&self, key: &Key) -> Result<Metadata> {
        for store in self {
            match store.getmeta(key) {
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

    fn getmissing(&self, keys: &[Key]) -> Result<Vec<Key>> {
        let initial_keys = Ok(keys.iter().map(|k| k.clone()).collect());
        self.into_iter()
            .fold(initial_keys, |missing_keys, store| match missing_keys {
                Ok(missing_keys) => store.getmissing(&missing_keys),
                Err(e) => Err(e),
            })
    }
}
