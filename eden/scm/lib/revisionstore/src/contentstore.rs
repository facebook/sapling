/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{format_err, Result};
use bytes::Bytes;
use tracing::info_span;

use configparser::{
    config::ConfigSet,
    hg::{ByteCount, ConfigSetHgExt},
};
use types::Key;

use crate::{
    datastore::{
        strip_metadata, ContentDataStore, ContentMetadata, Delta, HgIdDataStore,
        HgIdMutableDeltaStore, Metadata, RemoteDataStore,
    },
    indexedlogdatastore::IndexedLogHgIdDataStore,
    lfs::{LfsMultiplexer, LfsRemote, LfsStore},
    localstore::LocalStore,
    memcache::MemcacheStore,
    multiplexstore::MultiplexDeltaStore,
    packstore::{CorruptionPolicy, MutableDataPackStore},
    remotestore::HgIdRemoteStore,
    repack::RepackLocation,
    types::StoreKey,
    uniondatastore::{UnionContentDataStore, UnionHgIdDataStore},
    util::{
        get_cache_packs_path, get_cache_path, get_indexedlogdatastore_path, get_local_path,
        get_packs_path,
    },
};

/// A `ContentStore` aggregate all the local and remote stores and expose them as one. Both local and
/// remote stores can be queried and accessed via the `HgIdDataStore` trait. The local store can also
/// be written to via the `HgIdMutableDeltaStore` trait, this is intended to be used to store local
/// commit data.
pub struct ContentStore {
    datastore: UnionHgIdDataStore<Arc<dyn HgIdDataStore>>,
    local_mutabledatastore: Option<Arc<dyn HgIdMutableDeltaStore>>,
    shared_mutabledatastore: Arc<dyn HgIdMutableDeltaStore>,
    remote_store: Option<Arc<dyn RemoteDataStore>>,

    blob_stores: UnionContentDataStore<Arc<dyn ContentDataStore>>,
}

impl ContentStore {
    pub fn new(local_path: impl AsRef<Path>, config: &ConfigSet) -> Result<Self> {
        ContentStoreBuilder::new(config)
            .local_path(&local_path)
            .build()
    }

    /// Some blobs may contain copy-from metadata, let's strip it. For more details about the
    /// copy-from metadata, see `datastore::strip_metadata`.
    ///
    /// XXX: This should only be used on `ContentStore` that are storing actual
    /// file content, tree stores should use the `get` method instead.
    pub fn get_file_content(&self, key: &Key) -> Result<Option<Bytes>> {
        if let Some(vec) = self.get(key)? {
            let bytes = vec.into();
            let (bytes, _) = strip_metadata(&bytes)?;
            Ok(Some(bytes))
        } else {
            Ok(None)
        }
    }
}

// Repack specific methods, not to be used directly but by the repack code.
impl ContentStore {
    pub(crate) fn add_pending(
        &self,
        key: &Key,
        data: Bytes,
        meta: Metadata,
        location: RepackLocation,
    ) -> Result<()> {
        let delta = Delta {
            data,
            base: None,
            key: key.clone(),
        };

        match location {
            RepackLocation::Local => self.add(&delta, &meta),
            RepackLocation::Shared => self.shared_mutabledatastore.add(&delta, &meta),
        }
    }

    pub(crate) fn commit_pending(&self, location: RepackLocation) -> Result<Option<PathBuf>> {
        match location {
            RepackLocation::Local => self.flush(),
            RepackLocation::Shared => self.shared_mutabledatastore.flush(),
        }
    }
}

impl HgIdDataStore for ContentStore {
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>> {
        self.datastore.get(key)
    }

    fn get_meta(&self, key: &Key) -> Result<Option<Metadata>> {
        self.datastore.get_meta(key)
    }
}

impl RemoteDataStore for ContentStore {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<()> {
        if let Some(remote_store) = self.remote_store.as_ref() {
            let missing = self.get_missing(keys)?;
            if missing == vec![] {
                Ok(())
            } else {
                remote_store.prefetch(&missing)
            }
        } else {
            // There is no remote store, let's pretend everything is fine.
            Ok(())
        }
    }

    fn upload(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        if let Some(remote_store) = self.remote_store.as_ref() {
            remote_store.upload(keys)
        } else {
            Ok(keys.to_vec())
        }
    }
}

impl LocalStore for ContentStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        let span = info_span!("Get Missing", keys = keys.len(),);
        span.in_scope(|| self.datastore.get_missing(keys))
    }
}

impl Drop for ContentStore {
    /// The shared store is a cache, so let's flush all pending data when the `ContentStore` goes
    /// out of scope.
    fn drop(&mut self) {
        let _ = self.shared_mutabledatastore.flush();
    }
}

/// HgIdMutableDeltaStore is only implemented for the local store and not for the remote ones. The
/// remote stores will be automatically written to while calling the various `HgIdDataStore` methods.
///
/// These methods can only be used when the ContentStore was created with a local store.
impl HgIdMutableDeltaStore for ContentStore {
    /// Add the data to the local store.
    fn add(&self, delta: &Delta, metadata: &Metadata) -> Result<()> {
        self.local_mutabledatastore
            .as_ref()
            .ok_or_else(|| format_err!("writing to a non-local ContentStore is not allowed"))?
            .add(delta, metadata)
    }

    /// Commit the data written to the local store.
    fn flush(&self) -> Result<Option<PathBuf>> {
        self.local_mutabledatastore
            .as_ref()
            .ok_or_else(|| format_err!("flushing a non-local ContentStore is not allowed"))?
            .flush()
    }
}

impl ContentDataStore for ContentStore {
    /// Fetch a raw blob from the LFS stores.
    fn blob(&self, key: &StoreKey) -> Result<Option<Bytes>> {
        self.blob_stores.blob(key)
    }

    fn metadata(&self, key: &StoreKey) -> Result<Option<ContentMetadata>> {
        self.blob_stores.metadata(key)
    }
}

/// Builder for `ContentStore`. An `impl AsRef<Path>` represents the path to the store and a
/// `ConfigSet` of the Mercurial configuration are required to build a `ContentStore`. Users can
/// use this builder to add optional `HgIdRemoteStore` to enable remote data fetching， and a `Path`
/// suffix to specify other type of stores.
pub struct ContentStoreBuilder<'a> {
    local_path: Option<PathBuf>,
    no_local_store: bool,
    config: &'a ConfigSet,
    remotestore: Option<Arc<dyn HgIdRemoteStore>>,
    suffix: Option<PathBuf>,
    memcachestore: Option<Arc<MemcacheStore>>,
}

impl<'a> ContentStoreBuilder<'a> {
    pub fn new(config: &'a ConfigSet) -> Self {
        Self {
            local_path: None,
            no_local_store: false,
            config,
            remotestore: None,
            memcachestore: None,
            suffix: None,
        }
    }

    /// Path to the local store.
    pub fn local_path(mut self, local_path: impl AsRef<Path>) -> Self {
        self.local_path = Some(local_path.as_ref().to_path_buf());
        self
    }

    /// Allows a ContentStore to be created without a local store.
    ///
    /// This should be used in very specific cases that do not want a local store. Unless you know
    /// exactly that this is what you want, do not use.
    pub fn no_local_store(mut self) -> Self {
        self.no_local_store = true;
        self
    }

    pub fn remotestore(mut self, remotestore: Arc<dyn HgIdRemoteStore>) -> Self {
        self.remotestore = Some(remotestore);
        self
    }

    pub fn memcachestore(mut self, memcachestore: Arc<MemcacheStore>) -> Self {
        self.memcachestore = Some(memcachestore);
        self
    }

    pub fn suffix(mut self, suffix: impl AsRef<Path>) -> Self {
        self.suffix = Some(suffix.as_ref().to_path_buf());
        self
    }

    pub fn build(self) -> Result<ContentStore> {
        let local_path = get_local_path(&self.local_path, &self.suffix)?;
        let cache_path = get_cache_path(self.config, &self.suffix)?;

        let cache_packs_path = get_cache_packs_path(self.config, &self.suffix)?;
        let shared_pack_store = Arc::new(MutableDataPackStore::new(
            &cache_packs_path,
            CorruptionPolicy::REMOVE,
        )?);
        let mut datastore: UnionHgIdDataStore<Arc<dyn HgIdDataStore>> = UnionHgIdDataStore::new();

        if self
            .config
            .get_or_default::<bool>("remotefilelog", "indexedlogdatastore")?
        {
            let shared_indexedlogdatastore = Arc::new(IndexedLogHgIdDataStore::new(
                get_indexedlogdatastore_path(&cache_path)?,
            )?);
            datastore.add(shared_indexedlogdatastore);
        }

        let mut blob_stores: UnionContentDataStore<Arc<dyn ContentDataStore>> =
            UnionContentDataStore::new();

        // The shared stores should precede the local one since we expect both the number of blobs,
        // and the number of requests satisfied by the shared cache to be significantly higher than
        // ones in the local store.

        let enable_lfs = self.config.get_or_default::<bool>("remotefilelog", "lfs")?;

        let lfs_threshold = if enable_lfs {
            self.config.get_opt::<ByteCount>("lfs", "threshold")?
        } else {
            None
        };

        let shared_lfs_store = Arc::new(LfsStore::shared(&cache_path, self.config)?);
        blob_stores.add(shared_lfs_store.clone());

        let shared_store: Arc<dyn HgIdMutableDeltaStore> =
            if let Some(lfs_threshold) = lfs_threshold {
                let lfs_store = Arc::new(LfsMultiplexer::new(
                    shared_lfs_store.clone(),
                    shared_pack_store,
                    lfs_threshold.value() as usize,
                ));

                datastore.add(lfs_store.clone());

                lfs_store
            } else {
                datastore.add(shared_pack_store.clone());
                datastore.add(shared_lfs_store.clone());
                shared_pack_store
            };

        let (local_mutabledatastore, local_lfs_store): (Option<Arc<dyn HgIdMutableDeltaStore>>, _) =
            if let Some(unsuffixed_local_path) = self.local_path {
                let local_pack_store = Arc::new(MutableDataPackStore::new(
                    get_packs_path(&unsuffixed_local_path, &self.suffix)?,
                    CorruptionPolicy::IGNORE,
                )?);

                let local_lfs_store = Arc::new(LfsStore::local(&local_path.unwrap(), self.config)?);
                blob_stores.add(local_lfs_store.clone());

                let local_store: Arc<dyn HgIdMutableDeltaStore> =
                    if let Some(lfs_threshold) = lfs_threshold {
                        let local_store = Arc::new(LfsMultiplexer::new(
                            local_lfs_store.clone(),
                            local_pack_store,
                            lfs_threshold.value() as usize,
                        ));

                        datastore.add(local_store.clone());

                        local_store
                    } else {
                        datastore.add(local_pack_store.clone());
                        datastore.add(local_lfs_store.clone());
                        local_pack_store
                    };

                (Some(local_store), Some(local_lfs_store))
            } else {
                if !self.no_local_store {
                    return Err(format_err!(
                        "a ContentStore cannot be built without a local store"
                    ));
                }
                (None, None)
            };

        let remote_store: Option<Arc<dyn RemoteDataStore>> =
            if let Some(remotestore) = self.remotestore {
                let (cache, shared_store) = if let Some(memcachestore) = self.memcachestore {
                    // Combine the memcache store with the other stores. The intent is that all
                    // remote requests will first go to the memcache store, and only reach the
                    // slower remote store after that.
                    //
                    // If data isn't found in the memcache store, once fetched from the remote
                    // store it will be written to the local cache, and will populate the memcache
                    // store, so other clients and future requests won't need to go to a network
                    // store.
                    let memcachedatastore = memcachestore.clone().datastore(shared_store.clone());

                    let mut multiplexstore: MultiplexDeltaStore<Arc<dyn HgIdMutableDeltaStore>> =
                        MultiplexDeltaStore::new();
                    multiplexstore.add_store(memcachestore);
                    multiplexstore.add_store(shared_store.clone());

                    (
                        Some(memcachedatastore),
                        Arc::new(multiplexstore) as Arc<dyn HgIdMutableDeltaStore>,
                    )
                } else {
                    (None, shared_store.clone())
                };

                let mut remotestores = UnionHgIdDataStore::new();

                // First, the fast memcache store
                if let Some(cache) = cache {
                    remotestores.add(cache.clone());
                };

                // Second, the slower remotestore. For LFS blobs, the LFS pointers will be fetched
                // at this step and be written to the LFS store.
                let filenode_remotestore = remotestore.datastore(shared_store.clone());
                remotestores.add(filenode_remotestore.clone());

                // Third, the LFS remote store. The previously fetched LFS pointers will be used to
                // fetch the actual blobs in this store.
                if enable_lfs {
                    let lfs_remote_store = Arc::new(LfsRemote::new(
                        shared_lfs_store,
                        local_lfs_store,
                        self.config,
                    )?);
                    remotestores.add(lfs_remote_store.datastore(shared_store.clone()));

                    // Fallback store if the LFS one is dead. In `ContentStore::get_missing`, when
                    // the LFS pointers are available locally, a `StoreKey::Content` will be
                    // returned, preventing the first `filenode_remotestore` from trying to fetch
                    // the blob. However, in this situation, when the LFS server is down, the
                    // `LfsStore::get_missing` will return a `StoreKey::HgId`, that can then be
                    // fetched by the following store.
                    remotestores.add(filenode_remotestore);
                }

                let remotestores = Arc::new(remotestores);
                datastore.add(remotestores.clone());
                Some(remotestores)
            } else {
                None
            };

        Ok(ContentStore {
            datastore,
            local_mutabledatastore,
            shared_mutabledatastore: shared_store,
            remote_store,
            blob_stores,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    use bytes::Bytes;
    use tempfile::TempDir;

    use types::testutil::*;
    use util::path::create_dir;

    use crate::{
        metadatastore::MetadataStore,
        repack::{repack, RepackKind, RepackLocation},
        testutil::{make_config, make_lfs_config, FakeHgIdRemoteStore},
        types::ContentHash,
    };

    #[test]
    fn test_new() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let _store = ContentStore::new(&localdir, &config)?;
        Ok(())
    }

    #[test]
    fn test_add_get() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let store = ContentStore::new(&localdir, &config)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k1.clone(),
        };
        store.add(&delta, &Default::default())?;
        let stored = store.get(&k1)?;
        assert_eq!(stored.as_deref(), Some(delta.data.as_ref()));
        Ok(())
    }

    #[test]
    fn test_add_dropped() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let store = ContentStore::new(&localdir, &config)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: Some(key("a", "1")),
            key: k1.clone(),
        };
        store.add(&delta, &Default::default())?;
        drop(store);

        let store = ContentStore::new(&localdir, &config)?;
        assert!(store.get(&k1)?.is_none());
        Ok(())
    }

    #[test]
    fn test_add_flush_get() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let store = ContentStore::new(&localdir, &config)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k1.clone(),
        };
        store.add(&delta, &Default::default())?;
        store.flush()?;
        let stored = store.get(&k1)?;
        assert_eq!(stored.as_deref(), Some(delta.data.as_ref()));
        Ok(())
    }

    #[test]
    fn test_add_flush_drop_get() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let store = ContentStore::new(&localdir, &config)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k1.clone(),
        };
        store.add(&delta, &Default::default())?;
        store.flush()?;
        drop(store);

        let store = ContentStore::new(&localdir, &config)?;
        let stored = store.get(&k1)?;
        assert_eq!(stored.as_deref(), Some(delta.data.as_ref()));
        Ok(())
    }

    #[test]
    fn test_remote_store() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let k = key("a", "1");
        let data = Bytes::from(&[1, 2, 3, 4][..]);

        let mut map = HashMap::new();
        map.insert(k.clone(), (data.clone(), None));
        let mut remotestore = FakeHgIdRemoteStore::new();
        remotestore.data(map);

        let store = ContentStoreBuilder::new(&config)
            .local_path(&localdir)
            .remotestore(Arc::new(remotestore))
            .build()?;
        let data_get = store.get(&k)?;

        assert_eq!(data_get.unwrap(), data);
        Ok(())
    }

    #[test]
    fn test_remote_store_cached() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let k = key("a", "1");
        let data = Bytes::from(&[1, 2, 3, 4][..]);

        let mut map = HashMap::new();
        map.insert(k.clone(), (data.clone(), None));

        let mut remotestore = FakeHgIdRemoteStore::new();
        remotestore.data(map);

        let store = ContentStoreBuilder::new(&config)
            .local_path(&localdir)
            .remotestore(Arc::new(remotestore))
            .build()?;
        store.get(&k)?;
        drop(store);

        let store = ContentStore::new(&localdir, &config)?;
        let data_get = store.get(&k)?;

        assert_eq!(data_get.unwrap(), data);

        Ok(())
    }

    #[test]
    fn test_not_in_remote_store() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let map = HashMap::new();
        let mut remotestore = FakeHgIdRemoteStore::new();
        remotestore.data(map);

        let store = ContentStoreBuilder::new(&config)
            .local_path(&localdir)
            .remotestore(Arc::new(remotestore))
            .build()?;

        let k = key("a", "1");
        assert_eq!(store.get(&k)?, None);
        Ok(())
    }

    #[test]
    fn test_fetch_location() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let k = key("a", "1");
        let data = Bytes::from(&[1, 2, 3, 4][..]);

        let mut map = HashMap::new();
        map.insert(k.clone(), (data.clone(), None));

        let mut remotestore = FakeHgIdRemoteStore::new();
        remotestore.data(map);

        let store = ContentStoreBuilder::new(&config)
            .local_path(&localdir)
            .remotestore(Arc::new(remotestore))
            .build()?;
        store.get(&k)?;
        store.shared_mutabledatastore.get(&k)?;
        assert!(store
            .local_mutabledatastore
            .as_ref()
            .unwrap()
            .get(&k)?
            .is_none());
        Ok(())
    }

    #[test]
    fn test_add_shared_only_store() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let store = ContentStore::new(&localdir, &config)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: Some(key("a", "1")),
            key: k1.clone(),
        };
        store.add(&delta, &Default::default())?;
        store.flush()?;

        let store = ContentStoreBuilder::new(&config).no_local_store().build()?;
        assert_eq!(store.get(&k1)?, None);
        Ok(())
    }

    #[test]
    fn test_no_local_store() -> Result<()> {
        let cachedir = TempDir::new()?;
        let config = make_config(&cachedir);
        assert!(ContentStoreBuilder::new(&config).build().is_err());
        Ok(())
    }

    #[test]
    fn test_lfs_local() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let lfs_store = LfsStore::local(&localdir, &config)?;
        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k1.clone(),
        };
        lfs_store.add(&delta, &Default::default())?;
        lfs_store.flush()?;

        let store = ContentStore::new(&localdir, &config)?;
        assert_eq!(store.get(&k1)?, Some(delta.data.as_ref().to_vec()));
        Ok(())
    }

    #[test]
    fn test_lfs_shared() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let mut lfs_cache_dir = cachedir.path().to_path_buf();
        lfs_cache_dir.push("test");
        create_dir(&lfs_cache_dir)?;
        let lfs_store = LfsStore::shared(&lfs_cache_dir, &config)?;
        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k1.clone(),
        };
        lfs_store.add(&delta, &Default::default())?;
        lfs_store.flush()?;

        let store = ContentStore::new(&localdir, &config)?;
        assert_eq!(store.get(&k1)?, Some(delta.data.as_ref().to_vec()));
        Ok(())
    }

    #[test]
    fn test_lfs_blob() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_lfs_config(&cachedir);

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4, 5][..]),
            base: None,
            key: k1.clone(),
        };

        let store = ContentStore::new(&localdir, &config)?;
        store.add(&delta, &Default::default())?;

        let blob = store.blob(&StoreKey::from(k1))?;
        assert_eq!(blob, Some(delta.data));

        Ok(())
    }

    #[test]
    fn test_lfs_metadata() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_lfs_config(&cachedir);

        let k1 = key("a", "2");
        let data = Bytes::from(&[1, 2, 3, 4, 5][..]);
        let hash = ContentHash::sha256(&data);
        let delta = Delta {
            data,
            base: None,
            key: k1.clone(),
        };

        let store = ContentStore::new(&localdir, &config)?;
        store.add(&delta, &Default::default())?;

        let metadata = store.metadata(&StoreKey::from(k1))?;
        assert_eq!(
            metadata,
            Some(ContentMetadata {
                size: 5,
                is_binary: false,
                hash,
            })
        );

        Ok(())
    }

    #[test]
    fn test_lfs_multiplexer() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_lfs_config(&cachedir);

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4, 5][..]),
            base: None,
            key: k1.clone(),
        };

        let store = ContentStore::new(&localdir, &config)?;
        store.add(&delta, &Default::default())?;
        store.flush()?;

        let lfs_store = LfsStore::local(&localdir, &config)?;
        let stored = lfs_store.get(&k1)?;
        assert_eq!(stored.as_deref(), Some(delta.data.as_ref()));
        Ok(())
    }

    #[test]
    fn test_repack_one_datapack_lfs() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let mut config = make_lfs_config(&cachedir);
        config.set("lfs", "threshold", Some("10M"), &Default::default());

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4, 5][..]),
            base: None,
            key: k1.clone(),
        };

        let store = Arc::new(ContentStore::new(&localdir, &config)?);
        store.add(&delta, &Default::default())?;
        store.flush()?;

        let metadata = Arc::new(MetadataStore::new(&localdir, &config)?);

        repack(
            get_packs_path(&localdir, &None)?,
            Some((store, metadata)),
            RepackKind::Full,
            RepackLocation::Local,
            &config,
        )?;

        let store = Arc::new(ContentStore::new(&localdir, &config)?);
        let stored = store.get(&k1)?;
        assert_eq!(stored.as_deref(), Some(delta.data.as_ref()));
        Ok(())
    }

    #[cfg(feature = "fb")]
    mod fb_tests {
        use super::*;

        use std::str::FromStr;

        use types::Sha256;

        #[test]
        fn test_lfs_remote() -> Result<()> {
            let cachedir = TempDir::new()?;
            let localdir = TempDir::new()?;
            let config = make_lfs_config(&cachedir);

            let k = key("a", "1");
            let sha256 = Sha256::from_str(
                "fc613b4dfd6736a7bd268c8a0e74ed0d1c04a959f59dd74ef2874983fd443fc9",
            )?;
            let size = 6;

            let pointer = format!(
                "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\nx-is-binary 0\n",
                sha256.to_hex(),
                size
            );

            let data = Bytes::from(pointer);

            let mut map = HashMap::new();
            map.insert(k.clone(), (data, Some(0x2000)));
            let mut remotestore = FakeHgIdRemoteStore::new();
            remotestore.data(map);

            let store = ContentStoreBuilder::new(&config)
                .local_path(&localdir)
                .remotestore(Arc::new(remotestore))
                .build()?;

            let data = store.get(&k)?.map(|vec| Bytes::from(vec));

            assert_eq!(data, Some(Bytes::from(&b"master"[..])));

            Ok(())
        }

        #[test]
        fn test_lfs_fallback_on_missing_blob() -> Result<()> {
            let cachedir = TempDir::new()?;
            let localdir = TempDir::new()?;
            let config = make_lfs_config(&cachedir);

            let k = key("a", "1");
            // This should be a missing blob.
            let sha256 = Sha256::from_str(
                "0000000000000000000000000000000000000000000000000000000000000042",
            )?;
            let size = 4;

            let pointer = format!(
                "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\nx-is-binary 0\n",
                sha256.to_hex(),
                size
            );

            let data = Bytes::from("AAAA");

            let mut map = HashMap::new();
            map.insert(k.clone(), (data.clone(), None));
            let mut remotestore = FakeHgIdRemoteStore::new();
            remotestore.data(map);

            let store = ContentStoreBuilder::new(&config)
                .local_path(&localdir)
                .remotestore(Arc::new(remotestore))
                .build()?;

            let delta = Delta {
                data: Bytes::from(pointer),
                base: None,
                key: k.clone(),
            };

            // Add the pointer the the shared store, but not the blob
            store.shared_mutabledatastore.add(
                &delta,
                &Metadata {
                    size: None,
                    flags: Some(0x2000),
                },
            )?;

            assert_eq!(
                store.get_missing(&[StoreKey::from(k.clone())])?,
                vec![StoreKey::from(k.clone())]
            );
            store.prefetch(&[StoreKey::from(k.clone())])?;
            // Even though the blob was missing, we got it!
            assert_eq!(store.get_missing(&[StoreKey::from(k.clone())])?, vec![]);

            Ok(())
        }
    }

    #[cfg(all(fbcode_build, target_os = "linux"))]
    mod fbcode_tests {
        use super::*;

        use std::str::FromStr;

        use once_cell::sync::Lazy;

        use memcache::MockMemcache;
        use types::Sha256;

        static MOCK: Lazy<MockMemcache> = Lazy::new(|| MockMemcache::new());

        #[fbinit::test]
        fn test_memcache_get() -> Result<()> {
            let _mock = Lazy::force(&MOCK);

            let cachedir = TempDir::new()?;
            let localdir = TempDir::new()?;
            let config = make_config(&cachedir);

            let k = key("a", "1234");
            let data = Bytes::from(&[1, 2, 3, 4][..]);

            let mut map = HashMap::new();
            map.insert(k.clone(), (data.clone(), None));
            let mut remotestore = FakeHgIdRemoteStore::new();
            remotestore.data(map);

            let memcache = Arc::new(MemcacheStore::new(&config)?);
            let store = ContentStoreBuilder::new(&config)
                .local_path(&localdir)
                .remotestore(Arc::new(remotestore))
                .memcachestore(memcache.clone())
                .build()?;
            let data_get = store.get(&k)?;
            assert_eq!(data_get.unwrap(), data);

            loop {
                let memcache_data = memcache
                    .get_data_iter(&[k.clone()])
                    .collect::<Result<Vec<_>>>()?;
                if !memcache_data.is_empty() {
                    assert_eq!(memcache_data[0].data, data);
                    break;
                }
            }
            Ok(())
        }

        #[fbinit::test]
        fn test_memcache_get_large() -> Result<()> {
            let _mock = Lazy::force(&MOCK);

            let cachedir = TempDir::new()?;
            let localdir = TempDir::new()?;
            let config = make_config(&cachedir);

            let k = key("a", "abcd");

            let data = (0..10 * 1024 * 1024)
                .map(|_| rand::random::<u8>())
                .collect::<Bytes>();
            assert_eq!(data.len(), 10 * 1024 * 1024);

            let mut map = HashMap::new();
            map.insert(k.clone(), (data.clone(), None));
            let mut remotestore = FakeHgIdRemoteStore::new();
            remotestore.data(map);

            let memcache = Arc::new(MemcacheStore::new(&config)?);
            let store = ContentStoreBuilder::new(&config)
                .local_path(&localdir)
                .remotestore(Arc::new(remotestore))
                .memcachestore(memcache.clone())
                .build()?;
            let data_get = store.get(&k)?;
            assert_eq!(data_get.unwrap(), data);

            let memcache_data = memcache.get_data_iter(&[k]).collect::<Result<Vec<_>>>()?;
            assert_eq!(memcache_data, vec![]);
            Ok(())
        }

        #[fbinit::test]
        fn test_memcache_no_wait() -> Result<()> {
            let _mock = Lazy::force(&MOCK);

            let cachedir = TempDir::new()?;
            let localdir = TempDir::new()?;
            let mut config = make_config(&cachedir);
            config.set(
                "remotefilelog",
                "waitformemcache",
                Some("false"),
                &Default::default(),
            );

            let k = key("a", "1");
            let data = Bytes::from(&[1, 2, 3, 4][..]);

            let mut map = HashMap::new();
            map.insert(k.clone(), (data.clone(), None));
            let mut remotestore = FakeHgIdRemoteStore::new();
            remotestore.data(map);

            let memcache = Arc::new(MemcacheStore::new(&config)?);
            let store = ContentStoreBuilder::new(&config)
                .local_path(&localdir)
                .remotestore(Arc::new(remotestore))
                .memcachestore(memcache)
                .build()?;
            let data_get = store.get(&k)?;
            assert_eq!(data_get.unwrap(), data);

            // Ideally, we should check that we didn't wait for memcache, but that's timing
            // related and thus a bit hard to test.
            Ok(())
        }

        #[fbinit::test]
        fn test_memcache_lfs() -> Result<()> {
            let _mock = Lazy::force(&MOCK);

            let cachedir = TempDir::new()?;
            let localdir = TempDir::new()?;
            let config = make_lfs_config(&cachedir);

            let k = key("a", "1f5");
            let sha256 = Sha256::from_str(
                "fc613b4dfd6736a7bd268c8a0e74ed0d1c04a959f59dd74ef2874983fd443fc9",
            )?;
            let size = 6;

            let pointer = format!(
                "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\nx-is-binary 0\n",
                sha256.to_hex(),
                size
            );

            let pointer_data = Bytes::from(pointer);

            let mut map = HashMap::new();
            map.insert(k.clone(), (pointer_data.clone(), Some(0x2000)));
            let mut remotestore = FakeHgIdRemoteStore::new();
            remotestore.data(map);
            let remotestore = Arc::new(remotestore);

            let memcache = Arc::new(MemcacheStore::new(&config)?);
            let store = ContentStoreBuilder::new(&config)
                .local_path(&localdir)
                .remotestore(remotestore.clone())
                .memcachestore(memcache.clone())
                .build()?;

            let data = store.get(&k)?.map(Bytes::from);
            assert_eq!(data, Some(Bytes::from(&b"master"[..])));

            loop {
                let memcache_data = memcache
                    .get_data_iter(&[k.clone()])
                    .collect::<Result<Vec<_>>>()?;
                if !memcache_data.is_empty() {
                    assert_eq!(memcache_data[0].data, pointer_data);
                    break;
                }
            }

            let cachedir = TempDir::new()?;
            let localdir = TempDir::new()?;
            let config = make_lfs_config(&cachedir);

            let store = ContentStoreBuilder::new(&config)
                .local_path(&localdir)
                .remotestore(remotestore)
                .memcachestore(memcache)
                .build()?;

            let data = store.get(&k)?.map(Bytes::from);
            assert_eq!(data, Some(Bytes::from(&b"master"[..])));

            Ok(())
        }
    }
}
