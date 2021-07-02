/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use futures::prelude::*;

use async_runtime::block_on;
use progress::Unit;

use crate::{
    datastore::{HgIdDataStore, HgIdMutableDeltaStore, Metadata, RemoteDataStore, StoreResult},
    localstore::LocalStore,
    types::StoreKey,
};

use super::{hgid_keys, EdenApiRemoteStore, EdenApiStoreKind, File, Tree};

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

impl RemoteDataStore for EdenApiDataStore<File> {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        let client = self.remote.client.clone();
        let repo = self.remote.repo.clone();
        let progress = self.remote.progress.clone();
        let hgidkeys = hgid_keys(keys);

        let fetch = async move {
            let prog = progress.bar(
                "Downloading files over HTTP",
                Some(hgidkeys.len() as u64),
                Unit::Named("files"),
            )?;

            let mut response = File::prefetch_files(client, repo, hgidkeys, None).await?;
            while let Some(entry) = response.entries.try_next().await? {
                self.store.add_file(&entry)?;
                prog.increment(1)?;
            }
            self.store.get_missing(keys)
        };

        block_on(fetch)
    }

    fn upload(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        // XXX: EdenAPI does not presently support uploads.
        Ok(keys.to_vec())
    }
}

impl RemoteDataStore for EdenApiDataStore<Tree> {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        let client = self.remote.client.clone();
        let repo = self.remote.repo.clone();
        let progress = self.remote.progress.clone();
        let hgidkeys = hgid_keys(keys);

        let fetch = async move {
            let prog = progress.bar(
                "Downloading trees over HTTP",
                Some(hgidkeys.len() as u64),
                Unit::Named("trees"),
            )?;

            let mut response = Tree::prefetch_trees(client, repo, hgidkeys, None, None).await?;
            while let Some(Ok(entry)) = response.entries.try_next().await? {
                self.store.add_tree(&entry)?;
                prog.increment(1)?;
            }
            self.store.get_missing(keys)
        };

        block_on(fetch)
    }

    fn upload(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        // XXX: EdenAPI does not presently support uploads.
        Ok(keys.to_vec())
    }
}

impl HgIdDataStore for EdenApiDataStore<File> {
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        self.prefetch(&[key.clone()])?;
        self.store.get(key)
    }

    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        self.prefetch(&[key.clone()])?;
        self.store.get_meta(key)
    }

    fn refresh(&self) -> Result<()> {
        Ok(())
    }
}

impl HgIdDataStore for EdenApiDataStore<Tree> {
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        self.prefetch(&[key.clone()])?;
        self.store.get(key)
    }

    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        self.prefetch(&[key.clone()])?;
        self.store.get_meta(key)
    }

    fn refresh(&self) -> Result<()> {
        Ok(())
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

    use maplit::hashmap;
    use tempfile::TempDir;

    use configparser::config::ConfigSet;
    use types::testutil::*;

    use crate::{
        edenapi::{File, Tree},
        indexedlogdatastore::{IndexedLogDataStoreType, IndexedLogHgIdDataStore},
        localstore::ExtStoredPolicy,
        scmstore::{FileAttributes, FileStore, TreeStore},
        testutil::*,
    };

    #[test]
    fn test_get_file() -> Result<()> {
        // Set up mocked EdenAPI file and tree stores.
        let k = key("a", "def6f29d7b61f9cb70b2f14f79cd5c43c38e21b2");
        let d = delta("1234", None, k.clone());
        let files = hashmap! { k.clone() => d.data.clone() };

        let client = FakeEdenApi::new().files(files).into_arc();
        let remote_files = EdenApiRemoteStore::<File>::new("repo", client, None);

        // Set up local cache store to write received data.
        let mut store = FileStore::empty();

        let tmp = TempDir::new()?;
        let cache = Arc::new(IndexedLogHgIdDataStore::new(
            &tmp,
            ExtStoredPolicy::Ignore,
            &ConfigSet::new(),
            IndexedLogDataStoreType::Shared,
        )?);
        store.indexedlog_cache = Some(cache.clone());
        store.edenapi = Some(remote_files);

        // Attempt fetch.
        let mut fetched = store
            .fetch(std::iter::once(k.clone()), FileAttributes::CONTENT)
            .single()?
            .expect("key not found");
        assert_eq!(fetched.file_content()?.to_vec(), d.data.as_ref().to_vec());

        // Check that data was written to the local store.
        let mut fetched = cache.get_entry(k.clone())?.expect("key not found");
        assert_eq!(fetched.content()?.to_vec(), d.data.as_ref().to_vec());

        Ok(())
    }

    #[test]
    fn test_get_tree() -> Result<()> {
        // Set up mocked EdenAPI file and tree stores.
        let k = key("a", "def6f29d7b61f9cb70b2f14f79cd5c43c38e21b2");
        let d = delta("1234", None, k.clone());
        let trees = hashmap! { k.clone() => d.data.clone() };

        let client = FakeEdenApi::new().trees(trees).into_arc();
        let remote_trees = EdenApiRemoteStore::<Tree>::new("repo", client, None);

        // Set up local cache store to write received data.
        let mut store = TreeStore::empty();

        let tmp = TempDir::new()?;
        let cache = Arc::new(IndexedLogHgIdDataStore::new(
            &tmp,
            ExtStoredPolicy::Ignore,
            &ConfigSet::new(),
            IndexedLogDataStoreType::Shared,
        )?);
        store.indexedlog_cache = Some(cache.clone());
        store.edenapi = Some(remote_trees);

        // Attempt fetch.
        let mut fetched = store
            .fetch_batch(std::iter::once(k.clone()))?
            .complete
            .pop()
            .expect("key not found");
        assert_eq!(fetched.content()?.to_vec(), d.data.as_ref().to_vec());

        // Check that data was written to the local store.
        let mut fetched = cache.get_entry(k.clone())?.expect("key not found");
        assert_eq!(fetched.content()?.to_vec(), d.data.as_ref().to_vec());

        Ok(())
    }

    #[test]
    fn test_not_found() -> Result<()> {
        let client = FakeEdenApi::new().into_arc();
        let remote_trees = EdenApiRemoteStore::<Tree>::new("repo", client, None);

        // Set up local cache store to write received data.
        let mut store = TreeStore::empty();
        store.edenapi = Some(remote_trees);

        let k = key("a", "def6f29d7b61f9cb70b2f14f79cd5c43c38e21b2");

        // Attempt fetch.
        let fetched = store.fetch_batch(std::iter::once(k.clone()))?;
        assert_eq!(fetched.complete.len(), 0);
        assert_eq!(fetched.incomplete, vec![k]);

        Ok(())
    }
}
