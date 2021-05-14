/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{format_err, Result};
use minibytes::Bytes;
use regex::Regex;
use tracing::info_span;

use configparser::{config::ConfigSet, convert::ByteCount};
use hgtime::HgTime;
use types::{Key, RepoPathBuf};

use crate::{
    datastore::{
        strip_metadata, ContentDataStore, ContentMetadata, Delta, HgIdDataStore,
        HgIdMutableDeltaStore, Metadata, RemoteDataStore, ReportingRemoteDataStore, StoreResult,
    },
    indexedlogdatastore::{IndexedLogDataStoreType, IndexedLogHgIdDataStore},
    lfs::{LfsFallbackRemoteStore, LfsMultiplexer, LfsRemote, LfsStore},
    localstore::{ExtStoredPolicy, LocalStore},
    memcache::MemcacheStore,
    multiplexstore::MultiplexDeltaStore,
    packstore::{CorruptionPolicy, MutableDataPackStore},
    remotestore::HgIdRemoteStore,
    repack::RepackLocation,
    types::StoreKey,
    uniondatastore::{UnionContentDataStore, UnionHgIdDataStore},
    util::{
        check_run_once, get_cache_packs_path, get_cache_path, get_indexedlogdatastore_path,
        get_local_path, get_packs_path, RUN_ONCE_FILENAME,
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
    remote_store: Option<Arc<ReportingRemoteDataStore>>,

    blob_stores: UnionContentDataStore<Arc<dyn ContentDataStore>>,
}

impl ContentStore {
    pub fn new(local_path: impl AsRef<Path>, config: &ConfigSet) -> Result<Self> {
        ContentStoreBuilder::new(config)
            .local_path(&local_path)
            .build()
    }

    /// Attempt to repair the underlying stores that the `ContentStore` is comprised of.
    ///
    /// As this may violate some of the stores asumptions, care must be taken to call this only
    /// when no other `ContentStore` have been created for the `shared_path`.
    pub fn repair(
        shared_path: impl AsRef<Path>,
        local_path: Option<impl AsRef<Path>>,
        suffix: Option<impl AsRef<Path>>,
        config: &ConfigSet,
    ) -> Result<String> {
        let mut repair_str = String::new();
        let mut shared_path = shared_path.as_ref().to_path_buf();
        if let Some(suffix) = suffix.as_ref() {
            shared_path.push(suffix);
        }
        let local_path = get_local_path(
            &local_path.as_ref().map(|l| l.as_ref().to_path_buf()),
            &suffix.map(|p| p.as_ref().to_path_buf()),
        )?;

        repair_str += &IndexedLogHgIdDataStore::repair(
            get_indexedlogdatastore_path(&shared_path)?,
            config,
            IndexedLogDataStoreType::Shared,
        )?;
        if let Some(local_path) = local_path {
            repair_str += &IndexedLogHgIdDataStore::repair(
                get_indexedlogdatastore_path(local_path)?,
                config,
                IndexedLogDataStoreType::Local,
            )?;
        }
        repair_str += &LfsStore::repair(shared_path)?;

        Ok(repair_str)
    }

    /// Some blobs may contain copy-from metadata, let's strip it. For more details about the
    /// copy-from metadata, see `datastore::strip_metadata`.
    ///
    /// XXX: This should only be used on `ContentStore` that are storing actual
    /// file content, tree stores should use the `get` method instead.
    pub fn get_file_content(&self, key: &Key) -> Result<Option<Bytes>> {
        if let StoreResult::Found(vec) = self.get(StoreKey::hgid(key.clone()))? {
            let bytes = vec.into();
            let (bytes, _) = strip_metadata(&bytes)?;
            Ok(Some(bytes))
        } else {
            Ok(None)
        }
    }

    pub fn get_logged_fetches(&self) -> HashSet<RepoPathBuf> {
        if let Some(remote_store) = &self.remote_store {
            remote_store.take_seen()
        } else {
            HashSet::new()
        }
    }

    pub fn get_shared_mutable(&self) -> Arc<dyn HgIdMutableDeltaStore> {
        self.shared_mutabledatastore.clone()
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

    pub(crate) fn commit_pending(&self, location: RepackLocation) -> Result<Option<Vec<PathBuf>>> {
        match location {
            RepackLocation::Local => self.flush(),
            RepackLocation::Shared => self.shared_mutabledatastore.flush(),
        }
    }
}

impl HgIdDataStore for ContentStore {
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        self.datastore.get(key)
    }

    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        self.datastore.get_meta(key)
    }

    fn refresh(&self) -> Result<()> {
        self.datastore.refresh()
    }
}

impl RemoteDataStore for ContentStore {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        if let Some(remote_store) = self.remote_store.as_ref() {
            let missing = self.get_missing(keys)?;
            if missing == vec![] {
                Ok(vec![])
            } else {
                remote_store.prefetch(&missing)
            }
        } else {
            // There is no remote store, let's pretend everything is fine.
            Ok(vec![])
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
    fn flush(&self) -> Result<Option<Vec<PathBuf>>> {
        self.shared_mutabledatastore.as_ref().flush()?;
        self.local_mutabledatastore
            .as_ref()
            .ok_or_else(|| format_err!("flushing a non-local ContentStore is not allowed"))?
            .flush()
    }
}

impl ContentDataStore for ContentStore {
    /// Fetch a raw blob from the LFS stores.
    fn blob(&self, key: StoreKey) -> Result<StoreResult<Bytes>> {
        self.blob_stores.blob(key)
    }

    fn metadata(&self, key: StoreKey) -> Result<StoreResult<ContentMetadata>> {
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
    correlator: Option<String>,
    shared_indexedlog: Option<Arc<IndexedLogHgIdDataStore>>,
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
            correlator: None,
            shared_indexedlog: None,
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

    pub fn correlator(mut self, correlator: Option<impl ToString>) -> Self {
        self.correlator = correlator.map(|s| s.to_string());
        self
    }

    pub fn shared_indexedlog(mut self, indexedlog: Arc<IndexedLogHgIdDataStore>) -> Self {
        self.shared_indexedlog = Some(indexedlog);
        self
    }

    pub fn build(self) -> Result<ContentStore> {
        let local_path = get_local_path(&self.local_path, &self.suffix)?;
        let cache_path = get_cache_path(self.config, &self.suffix)?;
        check_cache_buster(&self.config, &cache_path);

        // Do this after the cache busting, since this will recreate the necessary directories.
        let cache_packs_path = get_cache_packs_path(self.config, &self.suffix)?;
        let max_pending_bytes = self
            .config
            .get_or("packs", "maxdatapendingbytes", || {
                // Default to 4GB
                ByteCount::from(4 * (1024 * 1024 * 1024))
            })?
            .value();
        let max_bytes = self
            .config
            .get_opt::<ByteCount>("packs", "maxdatabytes")?
            .map(|v| v.value());

        let mut datastore: UnionHgIdDataStore<Arc<dyn HgIdDataStore>> = UnionHgIdDataStore::new();
        let mut blob_stores: UnionContentDataStore<Arc<dyn ContentDataStore>> =
            UnionContentDataStore::new();

        let enable_lfs = self.config.get_or_default::<bool>("remotefilelog", "lfs")?;
        let extstored_policy = if enable_lfs {
            if self
                .config
                .get_or_default::<bool>("remotefilelog", "useextstored")?
            {
                ExtStoredPolicy::Use
            } else {
                ExtStoredPolicy::Ignore
            }
        } else {
            ExtStoredPolicy::Use
        };

        let shared_pack_store = Arc::new(MutableDataPackStore::new(
            &cache_packs_path,
            CorruptionPolicy::REMOVE,
            max_pending_bytes,
            max_bytes,
            extstored_policy,
        )?);
        let shared_indexedlogdatastore = if let Some(shared_indexedlog) = self.shared_indexedlog {
            shared_indexedlog
        } else {
            Arc::new(IndexedLogHgIdDataStore::new(
                get_indexedlogdatastore_path(&cache_path)?,
                extstored_policy,
                self.config,
                IndexedLogDataStoreType::Shared,
            )?)
        };

        // The shared stores should precede the local one since we expect both the number of blobs,
        // and the number of requests satisfied by the shared cache to be significantly higher than
        // ones in the local store.

        let lfs_threshold = if enable_lfs {
            self.config.get_opt::<ByteCount>("lfs", "threshold")?
        } else {
            None
        };

        let shared_lfs_store = Arc::new(LfsStore::shared(&cache_path, self.config)?);
        blob_stores.add(shared_lfs_store.clone());

        let primary: Arc<dyn HgIdMutableDeltaStore> =
            if self
                .config
                .get_or("remotefilelog", "write-hgcache-to-indexedlog", || true)?
            {
                // Put the indexedlog first, since recent data will have gone there.
                datastore.add(shared_indexedlogdatastore.clone());
                datastore.add(shared_pack_store);
                shared_indexedlogdatastore
            } else {
                datastore.add(shared_pack_store.clone());
                datastore.add(shared_indexedlogdatastore);
                shared_pack_store
            };
        datastore.add(shared_lfs_store.clone());

        let shared_mutabledatastore: Arc<dyn HgIdMutableDeltaStore> = {
            if let Some(lfs_threshold) = lfs_threshold {
                let lfs_store = Arc::new(LfsMultiplexer::new(
                    shared_lfs_store.clone(),
                    primary,
                    lfs_threshold.value() as usize,
                ));
                lfs_store
            } else {
                primary
            }
        };

        let (local_mutabledatastore, local_lfs_store): (Option<Arc<dyn HgIdMutableDeltaStore>>, _) =
            if let Some(unsuffixed_local_path) = self.local_path {
                let local_pack_store = Arc::new(MutableDataPackStore::new(
                    get_packs_path(&unsuffixed_local_path, &self.suffix)?,
                    CorruptionPolicy::IGNORE,
                    max_pending_bytes,
                    None,
                    extstored_policy,
                )?);
                let local_indexedlogdatastore = Arc::new(IndexedLogHgIdDataStore::new(
                    get_indexedlogdatastore_path(local_path.as_ref().unwrap())?,
                    extstored_policy,
                    self.config,
                    IndexedLogDataStoreType::Local,
                )?);

                let primary: Arc<dyn HgIdMutableDeltaStore> =
                    if self
                        .config
                        .get_or("remotefilelog", "write-local-to-indexedlog", || true)?
                    {
                        // Put the indexedlog first, since recent data will have gone there.
                        datastore.add(local_indexedlogdatastore.clone());
                        datastore.add(local_pack_store);
                        local_indexedlogdatastore
                    } else {
                        datastore.add(local_pack_store.clone());
                        datastore.add(local_indexedlogdatastore);
                        local_pack_store
                    };

                let local_lfs_store = Arc::new(LfsStore::local(&local_path.unwrap(), self.config)?);
                blob_stores.add(local_lfs_store.clone());
                datastore.add(local_lfs_store.clone());

                let local_mutabledatastore: Arc<dyn HgIdMutableDeltaStore> = {
                    if let Some(lfs_threshold) = lfs_threshold {
                        Arc::new(LfsMultiplexer::new(
                            local_lfs_store.clone(),
                            primary,
                            lfs_threshold.value() as usize,
                        ))
                    } else {
                        primary
                    }
                };

                (Some(local_mutabledatastore), Some(local_lfs_store))
            } else {
                if !self.no_local_store {
                    return Err(format_err!(
                        "a ContentStore cannot be built without a local store"
                    ));
                }
                (None, None)
            };

        let remote_store: Option<Arc<ReportingRemoteDataStore>> = if let Some(remotestore) =
            self.remotestore
        {
            let (cache, shared_store) = if let Some(memcachestore) = self.memcachestore {
                // Combine the memcache store with the other stores. The intent is that all
                // remote requests will first go to the memcache store, and only reach the
                // slower remote store after that.
                //
                // If data isn't found in the memcache store, once fetched from the remote
                // store it will be written to the local cache, and will populate the memcache
                // store, so other clients and future requests won't need to go to a network
                // store.
                let memcachedatastore = memcachestore
                    .clone()
                    .datastore(shared_mutabledatastore.clone());

                let mut multiplexstore: MultiplexDeltaStore<Arc<dyn HgIdMutableDeltaStore>> =
                    MultiplexDeltaStore::new();
                multiplexstore.add_store(memcachestore);
                multiplexstore.add_store(shared_mutabledatastore.clone());

                (
                    Some(memcachedatastore),
                    Arc::new(multiplexstore) as Arc<dyn HgIdMutableDeltaStore>,
                )
            } else {
                (None, shared_mutabledatastore.clone())
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
                    self.correlator,
                )?);
                remotestores.add(lfs_remote_store.datastore(shared_store.clone()));

                // Fallback store if the LFS one is dead.
                let lfs_fallback = LfsFallbackRemoteStore::new(filenode_remotestore);
                remotestores.add(lfs_fallback);
            }

            let remotestores: Box<dyn RemoteDataStore> = Box::new(remotestores);
            let logging_regex = self
                .config
                .get_opt::<String>("remotefilelog", "undesiredfileregex")?
                .map(|s| Regex::new(&s))
                .transpose()?;
            let remotestores = Arc::new(ReportingRemoteDataStore::new(remotestores, logging_regex));
            datastore.add(remotestores.clone());
            Some(remotestores)
        } else {
            None
        };

        Ok(ContentStore {
            datastore,
            local_mutabledatastore,
            shared_mutabledatastore,
            remote_store,
            blob_stores,
        })
    }
}

/// Reads the configs and deletes the hgcache if a hgcache-purge.$KEY=$DATE value hasn't already
/// been processed.
pub fn check_cache_buster(config: &ConfigSet, store_path: &Path) {
    for key in config.keys("hgcache-purge").into_iter() {
        if let Some(cutoff) = config
            .get("hgcache-purge", &key)
            .map(|c| HgTime::parse(&c))
            .flatten()
        {
            if check_run_once(store_path, &key, cutoff) {
                let _ = delete_hgcache(store_path);
                break;
            }
        }
    }
}

/// Recursively deletes the contents of the path, excluding the run-once marker file.
/// Ignores errors on individual files or directories.
fn delete_hgcache(store_path: &Path) -> Result<()> {
    for file in fs::read_dir(store_path)? {
        let _ = (|| -> Result<()> {
            let file = file?;
            if file.file_name() == RUN_ONCE_FILENAME {
                return Ok(());
            }

            let path = file.path();
            let file_type = file.file_type()?;
            if file_type.is_dir() {
                fs::remove_dir_all(path)?;
            } else if file_type.is_file() || file_type.is_symlink() {
                fs::remove_file(path)?;
            }
            Ok(())
        })();
    }
    Ok(())
}
