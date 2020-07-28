/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{collections::HashSet, sync::Arc};

use anyhow::Result;
use futures::prelude::*;

use crate::{
    datastore::{HgIdDataStore, HgIdMutableDeltaStore, Metadata, RemoteDataStore, StoreResult},
    localstore::LocalStore,
    types::StoreKey,
};

use super::{hgid_keys, EdenApiRemoteStore, EdenApiStoreKind};

/// A data store backed by an `EdenApiRemoteStore` and a mutable store.
///
/// Data will be fetched over the network via the remote store and stored in the
/// mutable store before being returned to the caller. This type is not exported
/// because it is intended to be used as a trait object.
pub(super) struct EdenApiDataStore<T> {
    remote: Arc<EdenApiRemoteStore<T>>,
    store: Arc<dyn HgIdMutableDeltaStore>,
}

impl<T: EdenApiStoreKind> EdenApiDataStore<T> {
    pub(super) fn new(
        remote: Arc<EdenApiRemoteStore<T>>,
        store: Arc<dyn HgIdMutableDeltaStore>,
    ) -> Self {
        Self { remote, store }
    }
}

impl<T: EdenApiStoreKind> RemoteDataStore for EdenApiDataStore<T> {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        let client = self.remote.client.clone();
        let repo = self.remote.repo.clone();
        let all_keys = keys.iter().cloned().collect::<HashSet<_>>();
        let keys = hgid_keys(keys);

        let fetch = async move {
            let mut response = T::prefetch(client, repo, keys, None).await?;
            let mut fetched = HashSet::new();
            while let Some(entry) = response.entries.try_next().await? {
                self.store.add_entry(&entry)?;
                fetched.insert(StoreKey::hgid(entry.key().clone()));
            }
            let not_fetched = &all_keys - &fetched;
            Ok(not_fetched.into_iter().collect::<Vec<_>>())
        };

        let mut rt = self.remote.runtime.lock();
        rt.block_on(fetch)
    }

    fn upload(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        // XXX: EdenAPI does not presently support uploads.
        Ok(keys.to_vec())
    }
}

impl<T: EdenApiStoreKind> HgIdDataStore for EdenApiDataStore<T> {
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        self.prefetch(&[key.clone()])?;
        self.store.get(key)
    }

    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        self.prefetch(&[key.clone()])?;
        self.store.get_meta(key)
    }
}

impl<T: EdenApiStoreKind> LocalStore for EdenApiDataStore<T> {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.store.get_missing(keys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    use maplit::hashmap;
    use tempfile::TempDir;

    use types::testutil::*;

    use crate::{
        edenapi::{File, Tree},
        indexedlogdatastore::IndexedLogHgIdDataStore,
        remotestore::HgIdRemoteStore,
        testutil::*,
    };

    #[test]
    fn test_get_file() -> Result<()> {
        // Set up mocked EdenAPI file and tree stores.
        let k = key("a", "1");
        let d = delta("1234", None, k.clone());
        let files = hashmap! { k.clone() => d.data.clone() };
        let trees = HashMap::new();

        let client = FakeEdenApi::new().files(files).trees(trees).into_arc();
        let remote_files = EdenApiRemoteStore::<File>::new("repo", client.clone())?;
        let remote_trees = EdenApiRemoteStore::<Tree>::new("repo", client.clone())?;

        // Set up local mutable store to write received data.
        let tmp = TempDir::new()?;
        let local = Arc::new(IndexedLogHgIdDataStore::new(&tmp)?);

        // Set up `EdenApiDataStore<File>`.
        let edenapi_files = remote_files.datastore(local.clone());

        // Attempt fetch.
        let k = StoreKey::hgid(k);
        let data = edenapi_files.get(k.clone())?;
        let meta = edenapi_files.get_meta(k.clone())?;
        assert_eq!(data, StoreResult::Found(d.data.as_ref().to_vec()));
        assert_eq!(
            meta,
            StoreResult::Found(Metadata {
                size: Some(d.data.len() as u64),
                flags: None
            })
        );

        // Check that data was written to the local store.
        let data = local.get(k.clone())?;
        let meta = local.get_meta(k.clone())?;
        assert_eq!(data, StoreResult::Found(d.data.as_ref().to_vec()));
        assert_eq!(
            meta,
            StoreResult::Found(Metadata {
                size: Some(d.data.len() as u64),
                flags: None
            })
        );

        // Using the same mock client, set up a store for trees.
        // Need to use a new local store since otherwise the key
        // would still be present locally from the previous fetch.
        let tmp = TempDir::new()?;
        let local = Arc::new(IndexedLogHgIdDataStore::new(&tmp)?);
        let edenapi_trees = remote_trees.datastore(local.clone());

        // Check that the same key cannot be accessed via the tree store.
        assert_eq!(edenapi_trees.get(k.clone())?, StoreResult::NotFound(k));

        Ok(())
    }

    #[test]
    fn test_get_tree() -> Result<()> {
        // Set up mocked EdenAPI file and tree stores.
        let k = key("a", "1");
        let d = delta("1234", None, k.clone());
        let files = HashMap::new();
        let trees = hashmap! { k.clone() => d.data.clone() };

        let client = FakeEdenApi::new().files(files).trees(trees).into_arc();
        let remote_files = EdenApiRemoteStore::<File>::new("repo", client.clone())?;
        let remote_trees = EdenApiRemoteStore::<Tree>::new("repo", client.clone())?;

        // Set up local mutable store to write received data.
        let tmp = TempDir::new()?;
        let local = Arc::new(IndexedLogHgIdDataStore::new(&tmp)?);

        // Set up `EdenApiDataStore<Tree>`.
        let edenapi_trees = remote_trees.datastore(local.clone());

        // Attempt fetch.
        let k = StoreKey::hgid(k);
        let data = edenapi_trees.get(k.clone())?;
        let meta = edenapi_trees.get_meta(k.clone())?;
        assert_eq!(data, StoreResult::Found(d.data.as_ref().to_vec()));
        assert_eq!(
            meta,
            StoreResult::Found(Metadata {
                size: Some(d.data.len() as u64),
                flags: None
            })
        );

        // Check that data was written to the local store.
        let data = local.get(k.clone())?;
        let meta = local.get_meta(k.clone())?;
        assert_eq!(data, StoreResult::Found(d.data.as_ref().to_vec()));
        assert_eq!(
            meta,
            StoreResult::Found(Metadata {
                size: Some(d.data.len() as u64),
                flags: None
            })
        );

        // Using the same mock client, set up a store for files.
        // Need to use a new local store since otherwise the key
        // would still be present locally from the previous fetch.
        let tmp = TempDir::new()?;
        let local = Arc::new(IndexedLogHgIdDataStore::new(&tmp)?);
        let edenapi_files = remote_files.datastore(local);

        // Check that the same key cannot be accessed via the file store.
        assert_eq!(edenapi_files.get(k.clone())?, StoreResult::NotFound(k));

        Ok(())
    }

    #[test]
    fn test_missing() -> Result<()> {
        // Set up empty EdenApi remote store.
        let client = FakeEdenApi::new().into_arc();
        let remote = EdenApiRemoteStore::<File>::new("repo", client)?;

        // Set up local mutable store.
        let tmp = TempDir::new()?;
        let store = Arc::new(IndexedLogHgIdDataStore::new(&tmp)?);

        // Set up `EdenApiDataStore`.
        let edenapi = remote.datastore(store.clone());

        let k = StoreKey::hgid(key("a", "1"));
        assert_eq!(edenapi.get(k.clone())?, StoreResult::NotFound(k));

        Ok(())
    }
}
