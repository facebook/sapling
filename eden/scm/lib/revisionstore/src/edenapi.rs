/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;

use edenapi::EdenApi;
use types::Key;

use crate::{
    datastore::{DataStore, Delta, Metadata, MutableDeltaStore, RemoteDataStore},
    historystore::{MutableHistoryStore, RemoteHistoryStore},
    localstore::LocalStore,
    remotestore::RemoteStore,
};

#[derive(Clone)]
enum EdenApiRemoteStoreKind {
    File,
    Tree,
}

/// Small shim around `EdenApi` that implements the `RemoteDataStore` and `DataStore` trait. All
/// the `DataStore` methods will always fetch data from the network.
#[derive(Clone)]
pub struct EdenApiRemoteStore {
    edenapi: Arc<Box<dyn EdenApi>>,
    kind: EdenApiRemoteStoreKind,
}

impl EdenApiRemoteStore {
    pub fn filestore(edenapi: Arc<Box<dyn EdenApi>>) -> Self {
        Self {
            edenapi,
            kind: EdenApiRemoteStoreKind::File,
        }
    }

    pub fn treestore(edenapi: Arc<Box<dyn EdenApi>>) -> Self {
        Self {
            edenapi,
            kind: EdenApiRemoteStoreKind::Tree,
        }
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
    fn prefetch(&self, keys: Vec<Key>) -> Result<()> {
        let edenapi = &self.inner.edenapi;
        let (entries, _) = match edenapi.kind {
            EdenApiRemoteStoreKind::File => edenapi.edenapi.get_files(keys, None)?,
            EdenApiRemoteStoreKind::Tree => edenapi.edenapi.get_trees(keys, None)?,
        };
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
    fn get(&self, _key: &Key) -> Result<Option<Vec<u8>>> {
        unreachable!();
    }

    fn get_delta(&self, key: &Key) -> Result<Option<Delta>> {
        match self.prefetch(vec![key.clone()]) {
            Ok(()) => self.inner.store.get_delta(key),
            Err(_) => Ok(None),
        }
    }

    fn get_delta_chain(&self, key: &Key) -> Result<Option<Vec<Delta>>> {
        match self.prefetch(vec![key.clone()]) {
            Ok(()) => self.inner.store.get_delta_chain(key),
            Err(_) => Ok(None),
        }
    }

    fn get_meta(&self, key: &Key) -> Result<Option<Metadata>> {
        match self.prefetch(vec![key.clone()]) {
            Ok(()) => self.inner.store.get_meta(key),
            Err(_) => Ok(None),
        }
    }
}

impl LocalStore for EdenApiRemoteDataStore {
    fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>> {
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
    fn test_get_delta() -> Result<()> {
        let tmp = TempDir::new()?;
        let store = IndexedLogDataStore::new(&tmp)?;

        let k = key("a", "1");
        let d = delta("1234", None, k.clone());

        let mut map = HashMap::new();
        map.insert(k.clone(), d.data.clone());

        let edenapi = EdenApiRemoteStore::filestore(Arc::new(fake_edenapi(map)));

        let remotestore = edenapi.datastore(Box::new(store.clone()));
        assert_eq!(remotestore.get_delta(&k)?.unwrap(), d);
        assert_eq!(store.get_delta(&k)?.unwrap(), d);

        Ok(())
    }

    #[test]
    fn test_missing() -> Result<()> {
        let tmp = TempDir::new()?;
        let store = IndexedLogDataStore::new(&tmp)?;

        let map = HashMap::new();
        let edenapi = EdenApiRemoteStore::filestore(Arc::new(fake_edenapi(map)));

        let remotestore = edenapi.datastore(Box::new(store.clone()));

        let k = key("a", "1");
        assert_eq!(remotestore.get_delta(&k)?, None);
        Ok(())
    }
}
