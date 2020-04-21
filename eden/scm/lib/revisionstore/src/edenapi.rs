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
    datastore::{Delta, HgIdDataStore, HgIdMutableDeltaStore, Metadata, RemoteDataStore},
    historystore::{HgIdMutableHistoryStore, RemoteHistoryStore},
    localstore::LocalStore,
    remotestore::HgIdRemoteStore,
    types::StoreKey,
};

#[derive(Clone)]
enum EdenApiHgIdRemoteStoreKind {
    File,
    Tree,
}

/// Small shim around `EdenApi` that implements the `RemoteDataStore` and `HgIdDataStore` trait. All
/// the `HgIdDataStore` methods will always fetch data from the network.
pub struct EdenApiHgIdRemoteStore {
    edenapi: Arc<dyn EdenApi>,
    kind: EdenApiHgIdRemoteStoreKind,
}

impl EdenApiHgIdRemoteStore {
    pub fn filestore(edenapi: Arc<dyn EdenApi>) -> Self {
        Self {
            edenapi,
            kind: EdenApiHgIdRemoteStoreKind::File,
        }
    }

    pub fn treestore(edenapi: Arc<dyn EdenApi>) -> Self {
        Self {
            edenapi,
            kind: EdenApiHgIdRemoteStoreKind::Tree,
        }
    }
}

impl HgIdRemoteStore for EdenApiHgIdRemoteStore {
    fn datastore(
        self: Arc<Self>,
        store: Arc<dyn HgIdMutableDeltaStore>,
    ) -> Arc<dyn RemoteDataStore> {
        Arc::new(EdenApiRemoteDataStore {
            edenapi: self,
            store,
        })
    }

    fn historystore(
        self: Arc<Self>,
        _store: Arc<dyn HgIdMutableHistoryStore>,
    ) -> Arc<dyn RemoteHistoryStore> {
        unimplemented!()
    }
}

struct EdenApiRemoteDataStore {
    edenapi: Arc<EdenApiHgIdRemoteStore>,
    store: Arc<dyn HgIdMutableDeltaStore>,
}

impl RemoteDataStore for EdenApiRemoteDataStore {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<()> {
        let edenapi = &self.edenapi;

        let keys = keys
            .iter()
            .filter_map(|k| match k {
                StoreKey::HgId(k) => Some(k.clone()),
                StoreKey::Content(_, _) => None,
            })
            .collect::<Vec<_>>();
        let (entries, _) = match edenapi.kind {
            EdenApiHgIdRemoteStoreKind::File => edenapi.edenapi.get_files(keys, None)?,
            EdenApiHgIdRemoteStoreKind::Tree => edenapi.edenapi.get_trees(keys, None)?,
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
            self.store.add(&delta, &metadata)?;
        }
        Ok(())
    }

    fn upload(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        Ok(keys.to_vec())
    }
}

impl HgIdDataStore for EdenApiRemoteDataStore {
    fn get(&self, _key: &Key) -> Result<Option<Vec<u8>>> {
        unreachable!();
    }

    fn get_delta(&self, key: &Key) -> Result<Option<Delta>> {
        match self.prefetch(&[StoreKey::hgid(key.clone())]) {
            Ok(()) => self.store.get_delta(key),
            Err(_) => Ok(None),
        }
    }

    fn get_delta_chain(&self, key: &Key) -> Result<Option<Vec<Delta>>> {
        match self.prefetch(&[StoreKey::hgid(key.clone())]) {
            Ok(()) => self.store.get_delta_chain(key),
            Err(_) => Ok(None),
        }
    }

    fn get_meta(&self, key: &Key) -> Result<Option<Metadata>> {
        match self.prefetch(&[StoreKey::hgid(key.clone())]) {
            Ok(()) => self.store.get_meta(key),
            Err(_) => Ok(None),
        }
    }
}

impl LocalStore for EdenApiRemoteDataStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.store.get_missing(keys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    use tempfile::TempDir;

    use types::testutil::*;

    use crate::{indexedlogdatastore::IndexedLogHgIdDataStore, testutil::*};

    #[test]
    fn test_get_delta() -> Result<()> {
        let tmp = TempDir::new()?;
        let store = Arc::new(IndexedLogHgIdDataStore::new(&tmp)?);

        let k = key("a", "1");
        let d = delta("1234", None, k.clone());

        let mut map = HashMap::new();
        map.insert(k.clone(), d.data.clone());

        let edenapi = Arc::new(EdenApiHgIdRemoteStore::filestore(fake_edenapi(map)));

        let remotestore = edenapi.datastore(store.clone());
        assert_eq!(remotestore.get_delta(&k)?.unwrap(), d);
        assert_eq!(store.get_delta(&k)?.unwrap(), d);

        Ok(())
    }

    #[test]
    fn test_missing() -> Result<()> {
        let tmp = TempDir::new()?;
        let store = Arc::new(IndexedLogHgIdDataStore::new(&tmp)?);

        let map = HashMap::new();
        let edenapi = Arc::new(EdenApiHgIdRemoteStore::filestore(fake_edenapi(map)));

        let remotestore = edenapi.datastore(store);

        let k = key("a", "1");
        assert_eq!(remotestore.get_delta(&k)?, None);
        Ok(())
    }
}
