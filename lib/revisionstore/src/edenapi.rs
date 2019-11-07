/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use failure::Fallible;

use edenapi::EdenApi;
use types::Key;

use crate::{
    datastore::{DataStore, Delta, Metadata, MutableDeltaStore, RemoteDataStore},
    historystore::{MutableHistoryStore, RemoteHistoryStore},
    localstore::LocalStore,
    remotestore::RemoteStore,
};

/// Small shim around `EdenApi` that implements the `RemoteDataStore` and `DataStore` trait. All
/// the `DataStore` methods will always fetch data from the network.
#[derive(Clone)]
pub struct EdenApiRemoteStore(Arc<Box<dyn EdenApi>>);

impl EdenApiRemoteStore {
    pub fn new(edenapi: Box<dyn EdenApi>) -> Self {
        Self(Arc::new(edenapi))
    }
}

impl RemoteStore for EdenApiRemoteStore {
    fn datastore(&self, store: Box<dyn MutableDeltaStore>) -> Arc<dyn RemoteDataStore> {
        Arc::new(EdenApiRemoteDataStore {
            inner: Arc::new(EdenApiRemoteDataStoreInner {
                edenapi: self.clone(),
                store,
            }),
        })
    }

    fn historystore(&self, _store: Box<dyn MutableHistoryStore>) -> Arc<dyn RemoteHistoryStore> {
        unimplemented!()
    }
}

struct EdenApiRemoteDataStoreInner {
    edenapi: EdenApiRemoteStore,
    store: Box<dyn MutableDeltaStore>,
}

#[derive(Clone)]
struct EdenApiRemoteDataStore {
    inner: Arc<EdenApiRemoteDataStoreInner>,
}

impl RemoteDataStore for EdenApiRemoteDataStore {
    fn prefetch(&self, keys: Vec<Key>) -> Fallible<()> {
        let (entries, _) = self.inner.edenapi.0.get_files(keys, None)?;
        for entry in entries {
            let key = entry.0.clone();
            let data = entry.1;
            let metadata = Metadata {
                size: Some(data.len() as u64),
                flags: None,
            };
            let delta = Delta {
                data,
                base: None,
                key,
            };
            self.inner.store.add(&delta, &metadata)?;
        }
        Ok(())
    }
}

impl DataStore for EdenApiRemoteDataStore {
    fn get(&self, _key: &Key) -> Fallible<Option<Vec<u8>>> {
        unreachable!();
    }

    fn get_delta(&self, key: &Key) -> Fallible<Option<Delta>> {
        match self.prefetch(vec![key.clone()]) {
            Ok(()) => self.inner.store.get_delta(key),
            Err(_) => Ok(None),
        }
    }

    fn get_delta_chain(&self, key: &Key) -> Fallible<Option<Vec<Delta>>> {
        match self.prefetch(vec![key.clone()]) {
            Ok(()) => self.inner.store.get_delta_chain(key),
            Err(_) => Ok(None),
        }
    }

    fn get_meta(&self, key: &Key) -> Fallible<Option<Metadata>> {
        match self.prefetch(vec![key.clone()]) {
            Ok(()) => self.inner.store.get_meta(key),
            Err(_) => Ok(None),
        }
    }
}

impl LocalStore for EdenApiRemoteDataStore {
    fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>> {
        Ok(keys.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    use tempfile::TempDir;

    use types::testutil::*;

    use crate::{indexedlogdatastore::IndexedLogDataStore, testutil::*};

    #[test]
    fn test_get_delta() -> Fallible<()> {
        let tmp = TempDir::new()?;
        let store = IndexedLogDataStore::new(&tmp)?;

        let k = key("a", "1");
        let d = delta("1234", None, k.clone());

        let mut map = HashMap::new();
        map.insert(k.clone(), d.data.clone());

        let edenapi = EdenApiRemoteStore::new(fake_edenapi(map));

        let remotestore = edenapi.datastore(Box::new(store.clone()));
        assert_eq!(remotestore.get_delta(&k)?.unwrap(), d);
        assert_eq!(store.get_delta(&k)?.unwrap(), d);

        Ok(())
    }

    #[test]
    fn test_missing() -> Fallible<()> {
        let tmp = TempDir::new()?;
        let store = IndexedLogDataStore::new(&tmp)?;

        let map = HashMap::new();
        let edenapi = EdenApiRemoteStore::new(fake_edenapi(map));

        let remotestore = edenapi.datastore(Box::new(store.clone()));

        let k = key("a", "1");
        assert_eq!(remotestore.get_delta(&k)?, None);
        Ok(())
    }
}
