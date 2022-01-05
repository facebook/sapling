/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use revisionstore::HgIdDataStore;
use revisionstore::HgIdMutableDeltaStore;
use revisionstore::HgIdMutableHistoryStore;
use revisionstore::HgIdRemoteStore;
use revisionstore::LocalStore;
use revisionstore::Metadata;
use revisionstore::RemoteDataStore;
use revisionstore::RemoteHistoryStore;
use revisionstore::StoreKey;
use revisionstore::StoreResult;

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
    fn prefetch(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        Ok(keys.to_vec())
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
    fn refresh(&self) -> Result<()> {
        Ok(())
    }
}

impl LocalStore for FakeRemoteDataStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.0.get_missing(keys)
    }
}
