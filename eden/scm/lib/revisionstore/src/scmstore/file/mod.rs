/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod fetch;
mod metrics;
mod types;

pub use self::{
    fetch::FileStoreFetch,
    metrics::{FileStoreMetrics, FileStoreWriteMetrics},
    types::{FileAttributes, FileAuxData, StoreFile},
};

pub(crate) use self::{fetch::FetchState, types::LazyFile};

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, bail, ensure, Result};
use parking_lot::RwLock;
use tracing::instrument;

use ::types::{Key, RepoPathBuf};
use minibytes::Bytes;

use crate::{
    datastore::{HgIdDataStore, HgIdMutableDeltaStore, RemoteDataStore},
    fetch_logger::FetchLogger,
    indexedlogauxstore::AuxStore,
    indexedlogdatastore::{Entry, IndexedLogHgIdDataStore, IndexedLogHgIdDataStoreWriteGuard},
    indexedlogutil::StoreType,
    lfs::{lfs_from_hg_file_blob, LfsRemote, LfsStore},
    memcache::MEMCACHE_DELAY,
    remotestore::HgIdRemoteStore,
    ContentDataStore, ContentMetadata, ContentStore, Delta, EdenApiFileStore, ExtStoredPolicy,
    LegacyStore, LocalStore, MemcacheStore, Metadata, MultiplexDeltaStore, RepackLocation,
    StoreKey, StoreResult,
};

pub struct FileStore {
    // Config
    // TODO(meyer): Move these to a separate config struct with default impl, etc.
    pub(crate) extstored_policy: ExtStoredPolicy,
    pub(crate) lfs_threshold_bytes: Option<u64>,
    pub(crate) cache_to_local_cache: bool,
    pub(crate) cache_to_memcache: bool,
    pub(crate) edenapi_retries: i32,
    /// Allow explicitly writing serialized LFS pointers outside of tests
    pub(crate) allow_write_lfs_ptrs: bool,
    pub(crate) prefer_computing_aux_data: bool,

    // Record remote fetches
    pub(crate) fetch_logger: Option<Arc<FetchLogger>>,

    // Local-only stores
    pub(crate) indexedlog_local: Option<Arc<IndexedLogHgIdDataStore>>,
    pub(crate) lfs_local: Option<Arc<LfsStore>>,

    // Local non-lfs cache aka shared store
    pub(crate) indexedlog_cache: Option<Arc<IndexedLogHgIdDataStore>>,

    // Local LFS cache aka shared store
    pub(crate) lfs_cache: Option<Arc<LfsStore>>,

    // Memcache
    pub(crate) memcache: Option<Arc<MemcacheStore>>,

    // Remote stores
    pub(crate) lfs_remote: Option<Arc<LfsRemote>>,
    pub(crate) edenapi: Option<Arc<EdenApiFileStore>>,

    // Legacy ContentStore fallback
    pub(crate) contentstore: Option<Arc<ContentStore>>,

    // Aux Data Stores
    pub(crate) aux_local: Option<Arc<AuxStore>>,
    pub(crate) aux_cache: Option<Arc<AuxStore>>,

    // Metrics, statistics, debugging
    pub(crate) metrics: Arc<RwLock<FileStoreMetrics>>,

    // Records the store creation time, so we can only use memcache for long running commands.
    pub(crate) creation_time: Instant,
}

impl Drop for FileStore {
    #[instrument(skip(self))]
    fn drop(&mut self) {
        let _ = self.flush();
    }
}
impl FileStore {
    #[instrument(skip(self, keys))]
    pub fn fetch(&self, keys: impl Iterator<Item = Key>, attrs: FileAttributes) -> FileStoreFetch {
        let mut state = FetchState::new(keys, attrs, &self);

        if let Some(ref aux_cache) = self.aux_cache {
            state.fetch_aux_indexedlog(aux_cache, StoreType::Shared);
        }

        if let Some(ref aux_local) = self.aux_local {
            state.fetch_aux_indexedlog(aux_local, StoreType::Local);
        }

        if let Some(ref indexedlog_cache) = self.indexedlog_cache {
            state.fetch_indexedlog(indexedlog_cache, StoreType::Shared);
        }

        if let Some(ref indexedlog_local) = self.indexedlog_local {
            state.fetch_indexedlog(indexedlog_local, StoreType::Local);
        }

        if let Some(ref lfs_cache) = self.lfs_cache {
            state.fetch_lfs(lfs_cache, StoreType::Shared);
        }

        if let Some(ref lfs_local) = self.lfs_local {
            state.fetch_lfs(lfs_local, StoreType::Local);
        }

        if self.use_memcache() {
            if let Some(ref memcache) = self.memcache {
                state.fetch_memcache(memcache);
            }
        }

        if self.prefer_computing_aux_data {
            state.derive_computable();
        }

        if let Some(ref edenapi) = self.edenapi {
            state.fetch_edenapi(edenapi);
        }

        if let Some(ref lfs_remote) = self.lfs_remote {
            state.fetch_lfs_remote(
                &lfs_remote.remote,
                self.lfs_local.clone(),
                self.lfs_cache.clone(),
            );
        }

        if let Some(ref contentstore) = self.contentstore {
            state.fetch_contentstore(contentstore);
        }

        state.derive_computable();

        state.write_to_cache(
            self.indexedlog_cache.as_ref().and_then(|s| {
                if self.cache_to_local_cache {
                    Some(s.as_ref())
                } else {
                    None
                }
            }),
            self.memcache.as_ref().and_then(|s| {
                if self.cache_to_memcache && self.use_memcache() {
                    Some(s.as_ref())
                } else {
                    None
                }
            }),
            self.aux_cache.as_ref().map(|s| s.as_ref()),
            self.aux_local.as_ref().map(|s| s.as_ref()),
        );

        let fetched = state.finish();
        self.metrics.write().fetch += fetched.metrics().clone();
        fetched
    }

    fn use_memcache(&self) -> bool {
        // Only use memcache if the process has been around a while. It takes 2s to setup, which
        // hurts responiveness for short commands.
        self.creation_time.elapsed() > MEMCACHE_DELAY
    }

    fn write_lfsptr(
        &self,
        indexedlog_local: &mut Option<IndexedLogHgIdDataStoreWriteGuard<'_>>,
        key: Key,
        bytes: Bytes,
        meta: Metadata,
    ) -> Result<()> {
        if !self.allow_write_lfs_ptrs {
            ensure!(
                std::env::var("TESTTMP").is_ok(),
                "writing LFS pointers directly is not allowed outside of tests"
            );
        }
        // TODO(meyer): We should try to eliminate directly writing LFS pointers, so we're only supporting it
        // via ContentStore for now.
        let contentstore = self.contentstore.as_ref().ok_or_else(|| {
            anyhow!("trying to write LFS pointer but no ContentStore is available")
        })?;
        let delta = Delta {
            data: bytes,
            base: None,
            key,
        };
        if let Some(indexedlog_local) = indexedlog_local.as_mut() {
            indexedlog_local.unlocked(|| contentstore.add(&delta, &meta))
        } else {
            contentstore.add(&delta, &meta)
        }?;

        Ok(())
    }

    fn write_lfs(&self, key: Key, bytes: Bytes) -> Result<()> {
        let lfs_local = self.lfs_local.as_ref().ok_or_else(|| {
            anyhow!("trying to write LFS file but no local LfsStore is available")
        })?;
        let (lfs_pointer, lfs_blob) = lfs_from_hg_file_blob(key.hgid, &bytes)?;
        let sha256 = lfs_pointer.sha256();

        // TODO(meyer): Do similar LockGuard impl for LfsStore so we can lock across the batch for both
        lfs_local.add_blob(&sha256, lfs_blob)?;
        lfs_local.add_pointer(lfs_pointer)?;

        Ok(())
    }

    fn write_nonlfs(
        &self,
        indexedlog_local: &mut Option<IndexedLogHgIdDataStoreWriteGuard<'_>>,
        key: Key,
        bytes: Bytes,
        meta: Metadata,
    ) -> Result<()> {
        let indexedlog_local = indexedlog_local.as_mut().ok_or_else(|| {
            anyhow!("trying to write non-LFS file but no local non-LFS IndexedLog is available")
        })?;
        indexedlog_local.put_entry(Entry::new(key, bytes, meta))?;

        Ok(())
    }

    #[instrument(skip(self, entries))]
    pub fn write_batch(&self, entries: impl Iterator<Item = (Key, Bytes, Metadata)>) -> Result<()> {
        // TODO(meyer): Don't fail the whole batch for a single write error.
        let mut metrics = FileStoreWriteMetrics::default();
        let mut indexedlog_local = self.indexedlog_local.as_ref().map(|l| l.write_lock());
        for (key, bytes, meta) in entries {
            if meta.is_lfs() {
                metrics.lfsptr.item(1);
                if let Err(e) = self.write_lfsptr(&mut indexedlog_local, key, bytes, meta) {
                    metrics.lfsptr.err(1);
                    return Err(e);
                }
                metrics.lfsptr.ok(1);
                continue;
            }
            let hg_blob_len = bytes.len() as u64;
            // Default to non-LFS if no LFS threshold is set
            if self
                .lfs_threshold_bytes
                .map_or(false, |threshold| hg_blob_len > threshold)
            {
                metrics.lfs.item(1);
                if let Err(e) = self.write_lfs(key, bytes) {
                    metrics.lfs.err(1);
                    return Err(e);
                }
                metrics.lfs.ok(1);
            } else {
                metrics.nonlfs.item(1);
                if let Err(e) = self.write_nonlfs(&mut indexedlog_local, key, bytes, meta) {
                    metrics.nonlfs.err(1);
                    return Err(e);
                }
                metrics.nonlfs.ok(1);
            }
        }
        self.metrics.write().write += metrics;
        Ok(())
    }

    #[instrument(skip(self))]
    pub fn local(&self) -> Self {
        FileStore {
            extstored_policy: self.extstored_policy.clone(),
            lfs_threshold_bytes: self.lfs_threshold_bytes.clone(),
            edenapi_retries: self.edenapi_retries.clone(),
            allow_write_lfs_ptrs: self.allow_write_lfs_ptrs,
            prefer_computing_aux_data: self.prefer_computing_aux_data,

            indexedlog_local: self.indexedlog_local.clone(),
            lfs_local: self.lfs_local.clone(),

            indexedlog_cache: self.indexedlog_cache.clone(),
            lfs_cache: self.lfs_cache.clone(),
            cache_to_local_cache: self.cache_to_local_cache.clone(),

            memcache: None,
            cache_to_memcache: self.cache_to_memcache.clone(),

            edenapi: None,
            lfs_remote: None,

            contentstore: None,
            fetch_logger: self.fetch_logger.clone(),
            metrics: self.metrics.clone(),

            aux_local: self.aux_local.clone(),
            aux_cache: self.aux_cache.clone(),

            creation_time: self.creation_time,
        }
    }

    #[allow(unused_must_use)]
    #[instrument(skip(self))]
    pub fn flush(&self) -> Result<()> {
        let mut result = Ok(());
        let mut handle_error = |error| {
            tracing::error!(%error);
            result = Err(error);
        };

        if let Some(ref indexedlog_local) = self.indexedlog_local {
            let span = tracing::info_span!("indexedlog_local");
            let _guard = span.enter();
            indexedlog_local.flush_log().map_err(&mut handle_error);
        }

        if let Some(ref indexedlog_cache) = self.indexedlog_cache {
            let span = tracing::info_span!("indexedlog_cache");
            let _guard = span.enter();
            indexedlog_cache.flush_log().map_err(&mut handle_error);
        }

        if let Some(ref lfs_local) = self.lfs_local {
            let span = tracing::info_span!("lfs_local");
            let _guard = span.enter();
            lfs_local.flush().map_err(&mut handle_error);
        }

        if let Some(ref lfs_cache) = self.lfs_cache {
            let span = tracing::info_span!("lfs_cache");
            let _guard = span.enter();
            lfs_cache.flush().map_err(&mut handle_error);
        }

        if let Some(ref aux_local) = self.aux_local {
            let span = tracing::info_span!("aux_local");
            let _guard = span.enter();
            aux_local.write().flush().map_err(&mut handle_error);
        }

        if let Some(ref aux_cache) = self.aux_cache {
            let span = tracing::info_span!("aux_cache");
            let _guard = span.enter();
            aux_cache.write().flush().map_err(&mut handle_error);
        }

        result
    }

    pub fn metrics(&self) -> Vec<(String, usize)> {
        self.metrics.read().metrics().collect()
    }

    pub fn empty() -> Self {
        FileStore {
            extstored_policy: ExtStoredPolicy::Ignore,
            lfs_threshold_bytes: None,
            edenapi_retries: 0,
            allow_write_lfs_ptrs: false,
            prefer_computing_aux_data: false,

            indexedlog_local: None,
            lfs_local: None,

            indexedlog_cache: None,
            lfs_cache: None,
            cache_to_local_cache: true,

            memcache: None,
            cache_to_memcache: true,

            edenapi: None,
            lfs_remote: None,

            contentstore: None,
            fetch_logger: None,
            metrics: FileStoreMetrics::new(),

            aux_local: None,
            aux_cache: None,

            creation_time: Instant::now(),
        }
    }
}

impl LegacyStore for FileStore {
    /// Returns only the local cache / shared stores, in place of the local-only stores, such that writes will go directly to the local cache.
    /// For compatibility with ContentStore::get_shared_mutable
    #[instrument(skip(self))]
    fn get_shared_mutable(&self) -> Arc<dyn HgIdMutableDeltaStore> {
        // this is infallible in ContentStore so panic if there are no shared/cache stores.
        assert!(
            self.indexedlog_cache.is_some() || self.lfs_cache.is_some(),
            "cannot get shared_mutable, no shared / local cache stores available"
        );
        Arc::new(FileStore {
            extstored_policy: self.extstored_policy.clone(),
            lfs_threshold_bytes: self.lfs_threshold_bytes.clone(),
            edenapi_retries: self.edenapi_retries.clone(),
            allow_write_lfs_ptrs: self.allow_write_lfs_ptrs,
            prefer_computing_aux_data: self.prefer_computing_aux_data,

            indexedlog_local: self.indexedlog_cache.clone(),
            lfs_local: self.lfs_cache.clone(),

            indexedlog_cache: None,
            lfs_cache: None,
            cache_to_local_cache: false,

            memcache: None,
            cache_to_memcache: false,

            edenapi: None,
            lfs_remote: None,

            contentstore: None,
            fetch_logger: self.fetch_logger.clone(),
            metrics: self.metrics.clone(),

            aux_local: None,
            aux_cache: None,

            creation_time: Instant::now(),
        })
    }

    fn get_logged_fetches(&self) -> HashSet<RepoPathBuf> {
        let mut seen = self
            .fetch_logger
            .as_ref()
            .map(|fl| fl.take_seen())
            .unwrap_or_default();
        if let Some(contentstore) = self.contentstore.as_ref() {
            seen.extend(contentstore.get_logged_fetches());
        }
        seen
    }

    #[instrument(skip(self))]
    fn get_file_content(&self, key: &Key) -> Result<Option<Bytes>> {
        self.metrics.write().api.hg_getfilecontent.call(0);
        self.fetch(std::iter::once(key.clone()), FileAttributes::CONTENT)
            .single()?
            .map(|entry| entry.content.unwrap().file_content())
            .transpose()
    }

    // If ContentStore is available, these call into ContentStore. Otherwise, implement these
    // methods on top of scmstore (though they should still eventaully be removed).
    fn add_pending(
        &self,
        key: &Key,
        data: Bytes,
        meta: Metadata,
        location: RepackLocation,
    ) -> Result<()> {
        self.metrics.write().api.hg_addpending.call(0);
        if let Some(contentstore) = self.contentstore.as_ref() {
            contentstore.add_pending(key, data, meta, location)
        } else {
            let delta = Delta {
                data,
                base: None,
                key: key.clone(),
            };

            match location {
                RepackLocation::Local => self.add(&delta, &meta),
                RepackLocation::Shared => self.get_shared_mutable().add(&delta, &meta),
            }
        }
    }

    fn commit_pending(&self, location: RepackLocation) -> Result<Option<Vec<PathBuf>>> {
        self.metrics.write().api.hg_commitpending.call(0);
        if let Some(contentstore) = self.contentstore.as_ref() {
            contentstore.commit_pending(location)
        } else {
            self.flush()?;
            Ok(None)
        }
    }
}

impl HgIdDataStore for FileStore {
    // Fetch the raw content of a single TreeManifest blob
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        self.metrics.write().api.hg_get.call(0);
        Ok(
            match self
                .fetch(
                    std::iter::once(key.clone()).filter_map(|sk| sk.maybe_into_key()),
                    FileAttributes::CONTENT,
                )
                .single()?
            {
                Some(entry) => StoreResult::Found(entry.content.unwrap().hg_content()?.into_vec()),
                None => StoreResult::NotFound(key),
            },
        )
    }

    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        self.metrics.write().api.hg_getmeta.call(0);
        Ok(
            match self
                .fetch(
                    std::iter::once(key.clone()).filter_map(|sk| sk.maybe_into_key()),
                    FileAttributes::CONTENT,
                )
                .single()?
            {
                Some(entry) => StoreResult::Found(entry.content.unwrap().metadata()?),
                None => StoreResult::NotFound(key),
            },
        )
    }

    fn refresh(&self) -> Result<()> {
        self.metrics.write().api.hg_refresh.call(0);
        // AFAIK refresh only matters for DataPack / PackStore
        Ok(())
    }
}

impl RemoteDataStore for FileStore {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.metrics.write().api.hg_prefetch.call(keys.len());
        Ok(self
            .fetch(
                keys.iter().cloned().filter_map(|sk| sk.maybe_into_key()),
                FileAttributes::CONTENT,
            )
            .missing()?
            .into_iter()
            .map(StoreKey::HgId)
            .collect())
    }

    fn upload(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.metrics.write().api.hg_upload.call(keys.len());
        // TODO(meyer): Eliminate usage of legacy API, or at least minimize it (do we really need memcache + multiplex, etc)
        if let Some(ref lfs_remote) = self.lfs_remote {
            let mut multiplex = MultiplexDeltaStore::new();
            multiplex.add_store(self.get_shared_mutable());
            if self.use_memcache() {
                if let Some(ref memcache) = self.memcache {
                    multiplex.add_store(memcache.clone());
                }
            }
            lfs_remote
                .clone()
                .datastore(Arc::new(multiplex))
                .upload(keys)
        } else {
            Ok(keys.to_vec())
        }
    }
}

impl LocalStore for FileStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.metrics.write().api.hg_getmissing.call(keys.len());
        Ok(self
            .local()
            .fetch(
                keys.iter().cloned().filter_map(|sk| sk.maybe_into_key()),
                FileAttributes::CONTENT,
            )
            .missing()?
            .into_iter()
            .map(StoreKey::HgId)
            .collect())
    }
}

impl HgIdMutableDeltaStore for FileStore {
    fn add(&self, delta: &Delta, metadata: &Metadata) -> Result<()> {
        self.metrics.write().api.hg_add.call(0);
        if let Delta {
            data,
            base: None,
            key,
        } = delta.clone()
        {
            self.write_batch(std::iter::once((key, data, metadata.clone())))
        } else {
            bail!("Deltas with non-None base are not supported")
        }
    }

    fn flush(&self) -> Result<Option<Vec<PathBuf>>> {
        self.metrics.write().api.hg_flush.call(0);
        self.flush()?;
        Ok(None)
    }
}

// TODO(meyer): Content addressing not supported at all for trees. I could look for HgIds present here and fetch with
// that if available, but I feel like there's probably something wrong if this is called for trees.
impl ContentDataStore for FileStore {
    fn blob(&self, key: StoreKey) -> Result<StoreResult<Bytes>> {
        self.metrics.write().api.contentdatastore_blob.call(0);
        Ok(
            match self
                .fetch(
                    std::iter::once(key.clone()).filter_map(|sk| sk.maybe_into_key()),
                    FileAttributes::CONTENT,
                )
                .single()?
            {
                Some(entry) => StoreResult::Found(entry.content.unwrap().file_content()?),
                None => StoreResult::NotFound(key),
            },
        )
    }

    fn metadata(&self, key: StoreKey) -> Result<StoreResult<ContentMetadata>> {
        self.metrics.write().api.contentdatastore_metadata.call(0);
        Ok(
            match self
                .fetch(
                    std::iter::once(key.clone()).filter_map(|sk| sk.maybe_into_key()),
                    FileAttributes::CONTENT,
                )
                .single()?
            {
                Some(StoreFile {
                    content: Some(LazyFile::Lfs(_blob, pointer)),
                    ..
                }) => StoreResult::Found(pointer.into()),
                Some(_) => StoreResult::NotFound(key),
                None => StoreResult::NotFound(key),
            },
        )
    }
}
