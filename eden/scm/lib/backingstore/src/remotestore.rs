/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use revisionstore::{
    HgIdDataStore, HgIdMutableDeltaStore, HgIdMutableHistoryStore, HgIdRemoteStore, LocalStore,
    Metadata, RemoteDataStore, RemoteHistoryStore, StoreKey, StoreResult,
};
use std::sync::Arc;

// TODO: Once we have EdenAPI production ready, remove this.
pub struct FakeRemoteStore;

pub struct FakeRemoteDataStore(Arc<dyn HgIdMutableDeltaStore>);

impl HgIdRemoteStore for FakeRemoteStore {
    fn datastore(
        self: Arc<Self>,
        store: Arc<dyn HgIdMutableDeltaStore>,
    ) -> Arc<dyn RemoteDataStore> {
        Arc::new(FakeRemoteDataStore(store))
    }

    fn historystore(
        self: Arc<Self>,
        _store: Arc<dyn HgIdMutableHistoryStore>,
    ) -> Arc<dyn RemoteHistoryStore> {
        unreachable!()
    }
}

impl RemoteDataStore for FakeRemoteDataStore {
    fn prefetch(&self, _keys: &[StoreKey]) -> Result<()> {
        Ok(())
    }

    fn upload(&self, _keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        unreachable!()
    }
}

impl HgIdDataStore for FakeRemoteDataStore {
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        Ok(StoreResult::NotFound(key))
    }
    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        Ok(StoreResult::NotFound(key))
    }
}

impl LocalStore for FakeRemoteDataStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.0.get_missing(keys)
    }
}
