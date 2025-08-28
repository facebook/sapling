/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;

use anyhow::Result;
use async_runtime::block_on;
use async_runtime::spawn_blocking;
use futures::prelude::*;
use progress_model::ProgressBar;
use tracing::field;
use types::FetchContext;

use super::File;
use super::SaplingRemoteApiRemoteStore;
use super::SaplingRemoteApiStoreKind;
use super::Tree;
use super::hgid_keys;
use crate::datastore::HgIdDataStore;
use crate::datastore::HgIdMutableDeltaStore;
use crate::datastore::RemoteDataStore;
use crate::datastore::StoreResult;
use crate::localstore::LocalStore;
use crate::types::StoreKey;
use crate::util;

/// A data store backed by an `SaplingRemoteApiRemoteStore` and a mutable store.
///
/// Data will be fetched over the network via the remote store and stored in the
/// mutable store before being returned to the caller. This type is not exported
/// because it is intended to be used as a trait object.
pub(super) struct SaplingRemoteApiDataStore<T> {
    remote: Arc<SaplingRemoteApiRemoteStore<T>>,
    store: Arc<dyn HgIdMutableDeltaStore>,
}

impl<T: SaplingRemoteApiStoreKind> SaplingRemoteApiDataStore<T> {
    pub(super) fn new(
        remote: Arc<SaplingRemoteApiRemoteStore<T>>,
        store: Arc<dyn HgIdMutableDeltaStore>,
    ) -> Self {
        Self { remote, store }
    }
}

impl RemoteDataStore for SaplingRemoteApiDataStore<File> {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        let client = self.remote.client.clone();
        let hgidkeys = hgid_keys(keys);

        let response = async move {
            let prog = ProgressBar::new_adhoc(
                "Downloading files over HTTP",
                hgidkeys.len() as u64,
                "files",
            );

            let fctx = FetchContext::sapling_prefetch();
            let response = File::prefetch_files(client, fctx, hgidkeys).await?;
            // store.add_file() may compress the data before writing it to the store. This can slow
            // things down enough that we don't pull responses off the queue fast enough and
            // edenapi starts queueing all the responses in memory. Let's write to the store in
            // parallel, so we have at least a few threads doing decompression for us.
            let mut entries = response
                .entries
                .map(|entry| {
                    let store = self.store.clone();
                    spawn_blocking(move || {
                        entry.map(|e| {
                            if let Ok(entry) = e.result {
                                store.add_file(&entry)
                            } else {
                                Ok(())
                            }
                        })
                    })
                })
                .buffer_unordered(4);

            while let Some(result) = entries.try_next().await? {
                let _ = result??;
                prog.increase_position(1);
            }
            // Explicitly force the result type here, since otherwise it can't infer the error
            // type.
            let result: Result<_> = Ok((self.store.get_missing(keys)?, response.stats.await?));
            result
        };

        let span = tracing::info_span!(
            "fetch_edenapi",
            downloaded = field::Empty,
            uploaded = field::Empty,
            requests = field::Empty,
            time = field::Empty,
            latency = field::Empty,
            download_speed = field::Empty,
            scmstore = false,
        );
        let _enter = span.enter();
        let (keys, stats) = block_on(response)?;
        util::record_edenapi_stats(&span, &stats);
        Ok(keys)
    }

    fn upload(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        // XXX: SaplingRemoteAPI does not presently support uploads.
        Ok(keys.to_vec())
    }
}

impl RemoteDataStore for SaplingRemoteApiDataStore<Tree> {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        let client = self.remote.client.clone();
        let hgidkeys = hgid_keys(keys);

        let response = async move {
            let prog = ProgressBar::new_adhoc(
                "Downloading trees over HTTP",
                hgidkeys.len() as u64,
                "trees",
            );

            let fctx = FetchContext::sapling_prefetch();
            let mut response = Tree::prefetch_trees(client, fctx, hgidkeys, None).await?;
            while let Some(Ok(entry)) = response.entries.try_next().await? {
                self.store.add_tree(&entry)?;
                prog.increase_position(1);
            }
            // Explicitly force the result type here, since otherwise it can't infer the error
            // type.
            let result: Result<_> = Ok((self.store.get_missing(keys)?, response.stats.await?));
            result
        };

        let span = tracing::info_span!(
            "fetch_edenapi",
            downloaded = field::Empty,
            uploaded = field::Empty,
            requests = field::Empty,
            time = field::Empty,
            latency = field::Empty,
            download_speed = field::Empty,
            scmstore = false,
        );
        let _enter = span.enter();
        let (keys, stats) = block_on(response)?;
        util::record_edenapi_stats(&span, &stats);
        Ok(keys)
    }

    fn upload(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        // XXX: SaplingRemoteAPI does not presently support uploads.
        Ok(keys.to_vec())
    }
}

impl HgIdDataStore for SaplingRemoteApiDataStore<File> {
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        self.prefetch(&[key.clone()])?;
        self.store.get(key)
    }

    fn refresh(&self) -> Result<()> {
        Ok(())
    }
}

impl HgIdDataStore for SaplingRemoteApiDataStore<Tree> {
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        self.prefetch(&[key.clone()])?;
        self.store.get(key)
    }

    fn refresh(&self) -> Result<()> {
        Ok(())
    }
}

impl<T: SaplingRemoteApiStoreKind> LocalStore for SaplingRemoteApiDataStore<T> {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        Ok(keys.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::str::FromStr;

    use edenapi_types::Sha1;
    use maplit::hashmap;
    use storemodel::SerializationFormat;
    use tempfile::TempDir;
    use types::Blake3;
    use types::FetchContext;
    use types::fetch_mode::FetchMode;
    use types::testutil::*;

    use super::*;
    use crate::edenapi::File;
    use crate::edenapi::Tree;
    use crate::indexedlogauxstore::AuxStore;
    use crate::indexedlogdatastore::IndexedLogHgIdDataStore;
    use crate::indexedlogdatastore::IndexedLogHgIdDataStoreConfig;
    use crate::indexedlogutil::StoreType;
    use crate::scmstore::FileAttributes;
    use crate::scmstore::FileAuxData;
    use crate::scmstore::FileStore;
    use crate::scmstore::TreeStore;
    use crate::scmstore::tree::types::TreeAttributes;
    use crate::testutil::*;

    #[test]
    fn test_get_file() -> Result<()> {
        // Set up mocked SaplingRemoteAPI file and tree stores.
        let k = key("a", "def6f29d7b61f9cb70b2f14f79cd5c43c38e21b2");
        let d = delta("1234", None, k.clone());
        let files = hashmap! { k.clone() => d.data.clone() };

        let client = FakeSaplingRemoteApi::new().files(files).into_arc();
        let remote_files = SaplingRemoteApiRemoteStore::<File>::new(client);

        // Set up local cache store to write received data.
        let mut store = FileStore::empty();

        let tmp = TempDir::new()?;
        let config = IndexedLogHgIdDataStoreConfig {
            max_log_count: None,
            max_bytes_per_log: None,
            max_bytes: None,
            btrfs_compression: false,
        };
        let cache = Arc::new(IndexedLogHgIdDataStore::new(
            &BTreeMap::<&str, &str>::new(),
            &tmp,
            &config,
            StoreType::Rotated,
            SerializationFormat::Hg,
        )?);
        store.indexedlog_cache = Some(cache.clone());
        store.edenapi = Some(remote_files);

        // Attempt fetch.
        let fetched = store
            .fetch(
                FetchContext::default(),
                std::iter::once(k.clone()),
                FileAttributes::CONTENT,
            )
            .single()?
            .expect("key not found");
        assert_eq!(
            fetched.file_content()?.into_bytes().to_vec(),
            d.data.as_ref().to_vec()
        );

        // Check that data was written to the local store.
        let fetched = cache.get_entry(&k.hgid)?.expect("key not found");
        assert_eq!(fetched.content()?.to_vec(), d.data.as_ref().to_vec());

        Ok(())
    }

    #[test]
    fn test_get_file_remote_only() -> Result<()> {
        // Set up mocked SaplingRemoteAPI file and tree stores.
        let k = key("a", "def6f29d7b61f9cb70b2f14f79cd5c43c38e21b2");
        let d = delta("1234", None, k.clone());
        let files = hashmap! { k.clone() => d.data.clone() };

        let client = FakeSaplingRemoteApi::new().files(files).into_arc();
        let remote_files = SaplingRemoteApiRemoteStore::<File>::new(client);

        // Set up local cache store to write received data.
        let mut store = FileStore::empty();

        let tmp = TempDir::new()?;
        let config = IndexedLogHgIdDataStoreConfig {
            max_log_count: None,
            max_bytes_per_log: None,
            max_bytes: None,
            btrfs_compression: false,
        };
        let cache = Arc::new(IndexedLogHgIdDataStore::new(
            &BTreeMap::<&str, &str>::new(),
            &tmp,
            &config,
            StoreType::Rotated,
            SerializationFormat::Hg,
        )?);
        store.indexedlog_cache = Some(cache.clone());
        store.edenapi = Some(remote_files);

        // Attempt fetch.
        let fetched = store
            .fetch(
                FetchContext::new(FetchMode::RemoteOnly),
                std::iter::once(k.clone()),
                FileAttributes::CONTENT,
            )
            .single()?
            .expect("key not found");
        assert_eq!(
            fetched.file_content()?.into_bytes().to_vec(),
            d.data.as_ref().to_vec()
        );

        // Check that data was written to the local store.
        let fetched = cache.get_entry(&k.hgid)?.expect("key not found");
        assert_eq!(fetched.content()?.to_vec(), d.data.as_ref().to_vec());

        Ok(())
    }

    #[test]
    fn test_get_tree() -> Result<()> {
        // Set up mocked SaplingRemoteAPI file and tree stores.
        let k = key("a", "def6f29d7b61f9cb70b2f14f79cd5c43c38e21b2");
        let d = delta("1234", None, k.clone());
        let trees = hashmap! { k.clone() => d.data.clone() };

        let client = FakeSaplingRemoteApi::new().trees(trees).into_arc();
        let remote_trees = SaplingRemoteApiRemoteStore::<Tree>::new(client);

        // Set up local cache store to write received data.
        let mut store = TreeStore::empty();

        let tmp = TempDir::new()?;
        let config = IndexedLogHgIdDataStoreConfig {
            max_log_count: None,
            max_bytes_per_log: None,
            max_bytes: None,
            btrfs_compression: false,
        };
        let cache = Arc::new(IndexedLogHgIdDataStore::new(
            &BTreeMap::<&str, &str>::new(),
            &tmp,
            &config,
            StoreType::Rotated,
            SerializationFormat::Hg,
        )?);
        store.indexedlog_cache = Some(cache.clone());
        store.edenapi = Some(remote_trees);

        // Attempt fetch.
        let mut fetched = store
            .fetch_batch(
                FetchContext::default(),
                std::iter::once(k.clone()),
                TreeAttributes::CONTENT,
            )
            .single()?
            .expect("key not found");
        assert_eq!(
            fetched.manifest_tree_entry()?.0.to_vec(),
            d.data.as_ref().to_vec()
        );

        // Check that data was written to the local store.
        let fetched = cache.get_entry(&k.hgid)?.expect("key not found");
        assert_eq!(fetched.content()?.to_vec(), d.data.as_ref().to_vec());

        Ok(())
    }

    #[test]
    fn test_get_tree_remote_only() -> Result<()> {
        // Set up mocked SaplingRemoteAPI file and tree stores.
        let k = key("a", "def6f29d7b61f9cb70b2f14f79cd5c43c38e21b2");
        let d = delta("1234", None, k.clone());
        let trees = hashmap! { k.clone() => d.data.clone() };

        let client = FakeSaplingRemoteApi::new().trees(trees).into_arc();
        let remote_trees = SaplingRemoteApiRemoteStore::<Tree>::new(client);

        // Set up local cache store to write received data.
        let mut store = TreeStore::empty();

        let tmp = TempDir::new()?;
        let config = IndexedLogHgIdDataStoreConfig {
            max_log_count: None,
            max_bytes_per_log: None,
            max_bytes: None,
            btrfs_compression: false,
        };
        let cache = Arc::new(IndexedLogHgIdDataStore::new(
            &BTreeMap::<&str, &str>::new(),
            &tmp,
            &config,
            StoreType::Rotated,
            SerializationFormat::Hg,
        )?);
        store.indexedlog_cache = Some(cache.clone());
        store.edenapi = Some(remote_trees);

        // Attempt fetch.
        let mut fetched = store
            .fetch_batch(
                FetchContext::new(FetchMode::RemoteOnly),
                std::iter::once(k.clone()),
                TreeAttributes::CONTENT,
            )
            .single()?
            .expect("key not found");
        assert_eq!(
            fetched.manifest_tree_entry()?.0.to_vec(),
            d.data.as_ref().to_vec()
        );

        // Check that data was written to the local store.
        let fetched = cache.get_entry(&k.hgid)?.expect("key not found");
        assert_eq!(fetched.content()?.to_vec(), d.data.as_ref().to_vec());

        Ok(())
    }

    #[test]
    fn test_not_found() -> Result<()> {
        let client = FakeSaplingRemoteApi::new().into_arc();
        let remote_trees = SaplingRemoteApiRemoteStore::<Tree>::new(client);

        // Set up local cache store to write received data.
        let mut store = TreeStore::empty();
        store.edenapi = Some(remote_trees);

        let k = key("a", "def6f29d7b61f9cb70b2f14f79cd5c43c38e21b2");

        // Attempt fetch.
        let fetched = store.fetch_batch(
            FetchContext::default(),
            std::iter::once(k.clone()),
            TreeAttributes::CONTENT,
        );
        let (found, missing, _errors) = fetched.consume();
        assert_eq!(found.len(), 0);
        assert_eq!(missing.into_keys().collect::<Vec<_>>(), vec![k]);

        Ok(())
    }

    #[test]
    fn test_get_aux_cache() -> Result<()> {
        // Set up mocked SaplingRemoteAPI file and tree stores.
        let k = key("a", "def6f29d7b61f9cb70b2f14f79cd5c43c38e21b2");
        let d = delta("1234", None, k.clone());
        let files = hashmap! { k.clone() => d.data };

        let client = FakeSaplingRemoteApi::new().files(files).into_arc();
        let remote_files = SaplingRemoteApiRemoteStore::<File>::new(client);

        // Set up local cache store to write received data.
        let mut store = FileStore::empty();
        store.edenapi = Some(remote_files);

        // Empty aux cache
        let tmp = TempDir::new()?;
        let aux_cache = Arc::new(AuxStore::new(&tmp, &empty_config(), StoreType::Rotated)?);
        store.aux_cache = Some(aux_cache);

        // Empty content cache
        let tmp = TempDir::new()?;
        let config = IndexedLogHgIdDataStoreConfig {
            max_log_count: None,
            max_bytes_per_log: None,
            max_bytes: None,
            btrfs_compression: false,
        };
        let cache = Arc::new(IndexedLogHgIdDataStore::new(
            &BTreeMap::<&str, &str>::new(),
            &tmp,
            &config,
            StoreType::Rotated,
            SerializationFormat::Hg,
        )?);
        store.indexedlog_cache = Some(cache.clone());

        let expected = FileAuxData {
            total_size: 4,
            sha1: Sha1::from_str("7110eda4d09e062aa5e4a390b0a572ac0d2c0220")?,
            blake3: Blake3::from_str(
                "2078b4229b5353de0268efc7f64b68f3c99fb8829e9c052117b4e1e090b2603a",
            )?,
            file_header_metadata: None,
        };

        // Test that we can read aux data from SaplingRemoteApi
        let fetched = store
            .fetch(
                FetchContext::default(),
                std::iter::once(k.clone()),
                FileAttributes::AUX,
            )
            .single()?
            .expect("key not found");
        assert_eq!(fetched.aux_data().expect("no aux data found"), expected);

        // Disable SaplingRemoteApi and local cache, make sure we can read from aux cache.
        store.edenapi = None;
        store.indexedlog_cache = None;
        let fetched = store
            .fetch(
                FetchContext::default(),
                std::iter::once(k.clone()),
                FileAttributes::AUX,
            )
            .single()?
            .expect("key not found");
        assert_eq!(fetched.aux_data().expect("no aux data found"), expected);

        // Content shouldn't have been cached
        assert_eq!(cache.get_entry(&k.hgid)?, None);

        Ok(())
    }
}
