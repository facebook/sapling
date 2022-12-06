/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod fetch;
mod metrics;
mod types;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use ::types::Key;
use ::types::RepoPathBuf;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::ensure;
use anyhow::Result;
use crossbeam::channel::unbounded;
use minibytes::Bytes;
use parking_lot::Mutex;
use parking_lot::RwLock;
use progress_model::AggregatingProgressBar;
use rand::Rng;

pub(crate) use self::fetch::FetchState;
pub use self::metrics::FileStoreFetchMetrics;
pub use self::metrics::FileStoreMetrics;
pub use self::metrics::FileStoreWriteMetrics;
pub use self::types::FileAttributes;
pub use self::types::FileAuxData;
pub(crate) use self::types::LazyFile;
pub use self::types::StoreFile;
use crate::datastore::HgIdDataStore;
use crate::datastore::HgIdMutableDeltaStore;
use crate::datastore::RemoteDataStore;
use crate::fetch_logger::FetchLogger;
use crate::indexedlogauxstore::AuxStore;
use crate::indexedlogdatastore::Entry;
use crate::indexedlogdatastore::IndexedLogHgIdDataStore;
use crate::indexedlogutil::StoreType;
use crate::lfs::lfs_from_hg_file_blob;
use crate::lfs::LfsPointersEntry;
use crate::lfs::LfsRemote;
use crate::lfs::LfsStore;
use crate::memcache::MEMCACHE_DELAY;
use crate::remotestore::HgIdRemoteStore;
use crate::scmstore::activitylogger::ActivityLogger;
use crate::scmstore::fetch::FetchMode;
use crate::scmstore::fetch::FetchResults;
use crate::ContentDataStore;
use crate::ContentMetadata;
use crate::ContentStore;
use crate::Delta;
use crate::EdenApiFileStore;
use crate::ExtStoredPolicy;
use crate::LegacyStore;
use crate::LocalStore;
use crate::MemcacheStore;
use crate::Metadata;
use crate::MultiplexDeltaStore;
use crate::RepackLocation;
use crate::StoreKey;
use crate::StoreResult;

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
    pub(crate) activity_logger: Option<Arc<Mutex<ActivityLogger>>>,
    pub(crate) metrics: Arc<RwLock<FileStoreMetrics>>,

    // Records the store creation time, so we can only use memcache for long running commands.
    pub(crate) creation_time: Instant,

    pub(crate) lfs_progress: Arc<AggregatingProgressBar>,

    // Don't flush on drop when we're using FileStore in a "disposable" context, like backingstore
    pub flush_on_drop: bool,
}

impl Drop for FileStore {
    fn drop(&mut self) {
        if self.flush_on_drop {
            let _ = self.flush();
        }
    }
}

impl FileStore {
    pub fn fetch(
        &self,
        keys: impl Iterator<Item = Key>,
        attrs: FileAttributes,
        fetch_mode: FetchMode,
    ) -> FetchResults<StoreFile> {
        let (found_tx, found_rx) = unbounded();
        let mut state = FetchState::new(keys, attrs, &self, found_tx);

        let keys_len = state.pending_len();

        let aux_cache = self.aux_cache.clone();
        let aux_local = self.aux_local.clone();
        let indexedlog_cache = self.indexedlog_cache.clone();
        let indexedlog_local = self.indexedlog_local.clone();
        let lfs_cache = self.lfs_cache.clone();
        let lfs_local = self.lfs_local.clone();
        let memcache = self.memcache.clone();
        let edenapi = self.edenapi.clone();
        let lfs_remote = self.lfs_remote.clone();
        let contentstore = self.contentstore.clone();
        let creation_time = self.creation_time;
        let prefer_computing_aux_data = self.prefer_computing_aux_data;
        let cache_to_memcache = self.cache_to_memcache;
        let metrics = self.metrics.clone();
        let activity_logger = self.activity_logger.clone();

        let process_func = move || {
            let start_instant = Instant::now();

            let all_keys: Vec<Key> = state.pending();
            let span = tracing::span!(
                tracing::Level::DEBUG,
                "file fetch",
                id = rand::thread_rng().gen::<u16>()
            );
            let _enter = span.enter();

            if let Some(ref aux_cache) = aux_cache {
                state.fetch_aux_indexedlog(aux_cache, StoreType::Shared);
            }

            if let Some(ref aux_local) = aux_local {
                state.fetch_aux_indexedlog(aux_local, StoreType::Local);
            }

            if let Some(ref indexedlog_cache) = indexedlog_cache {
                state.fetch_indexedlog(indexedlog_cache, StoreType::Shared);
            }

            if let Some(ref indexedlog_local) = indexedlog_local {
                state.fetch_indexedlog(indexedlog_local, StoreType::Local);
            }

            if let Some(ref lfs_cache) = lfs_cache {
                state.fetch_lfs(lfs_cache, StoreType::Shared);
            }

            if let Some(ref lfs_local) = lfs_local {
                state.fetch_lfs(lfs_local, StoreType::Local);
            }

            if let FetchMode::AllowRemote = fetch_mode {
                if use_memcache(creation_time) {
                    if let Some(ref memcache) = memcache {
                        state.fetch_memcache(
                            memcache,
                            indexedlog_cache.as_ref().map(|s| s.as_ref()),
                        );
                    }
                }
            }

            if prefer_computing_aux_data {
                state.derive_computable(
                    aux_cache.as_ref().map(|s| s.as_ref()),
                    aux_local.as_ref().map(|s| s.as_ref()),
                );
            }

            if let FetchMode::AllowRemote = fetch_mode {
                if let Some(ref edenapi) = edenapi {
                    state.fetch_edenapi(
                        edenapi,
                        indexedlog_cache.clone(),
                        lfs_cache.clone(),
                        aux_cache.clone(),
                        if cache_to_memcache && use_memcache(creation_time) {
                            memcache.clone()
                        } else {
                            None
                        },
                    );
                }
            }

            if let FetchMode::AllowRemote = fetch_mode {
                if let Some(ref lfs_remote) = lfs_remote {
                    state.fetch_lfs_remote(
                        &lfs_remote.remote,
                        lfs_local.clone(),
                        lfs_cache.clone(),
                    );
                }
            }

            if let FetchMode::AllowRemote = fetch_mode {
                if let Some(ref contentstore) = contentstore {
                    state.fetch_contentstore(contentstore);
                }
            }

            state.derive_computable(
                aux_cache.as_ref().map(|s| s.as_ref()),
                aux_local.as_ref().map(|s| s.as_ref()),
            );

            metrics.write().fetch += state.metrics().clone();
            state.finish();

            if let Some(activity_logger) = activity_logger {
                if let Err(err) =
                    activity_logger
                        .lock()
                        .log_file_fetch(all_keys, attrs, start_instant.elapsed())
                {
                    tracing::error!("Error writing activity log: {}", err);
                }
            }
        };

        // Only kick off a thread if there's a substantial amount of work.
        if keys_len > 1000 {
            std::thread::spawn(process_func);
        } else {
            process_func();
        }

        FetchResults::new(Box::new(found_rx.into_iter()))
    }

    fn write_lfsptr(&self, key: Key, bytes: Bytes) -> Result<()> {
        if !self.allow_write_lfs_ptrs {
            ensure!(
                std::env::var("TESTTMP").is_ok(),
                "writing LFS pointers directly is not allowed outside of tests"
            );
        }
        let lfs_local = self.lfs_local.as_ref().ok_or_else(|| {
            anyhow!("trying to write LFS pointer but no local LfsStore is available")
        })?;

        let lfs_pointer = LfsPointersEntry::from_bytes(bytes, key.hgid)?;
        lfs_local.add_pointer(lfs_pointer)
    }

    fn write_lfs(&self, key: Key, bytes: Bytes) -> Result<()> {
        let lfs_local = self.lfs_local.as_ref().ok_or_else(|| {
            anyhow!("trying to write LFS file but no local LfsStore is available")
        })?;
        let (lfs_pointer, lfs_blob) = lfs_from_hg_file_blob(key.hgid, &bytes)?;
        let sha256 = lfs_pointer.sha256();

        lfs_local.add_blob(&sha256, lfs_blob)?;
        lfs_local.add_pointer(lfs_pointer)?;

        Ok(())
    }

    fn write_nonlfs(&self, key: Key, bytes: Bytes, meta: Metadata) -> Result<()> {
        let indexedlog_local = self.indexedlog_local.as_ref().ok_or_else(|| {
            anyhow!("trying to write non-LFS file but no local non-LFS IndexedLog is available")
        })?;
        indexedlog_local.put_entry(Entry::new(key, bytes, meta))?;

        Ok(())
    }

    pub fn write_batch(&self, entries: impl Iterator<Item = (Key, Bytes, Metadata)>) -> Result<()> {
        // TODO(meyer): Don't fail the whole batch for a single write error.
        let mut metrics = FileStoreWriteMetrics::default();
        for (key, bytes, meta) in entries {
            if meta.is_lfs() {
                metrics.lfsptr.item(1);
                if let Err(e) = self.write_lfsptr(key, bytes) {
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
                if let Err(e) = self.write_nonlfs(key, bytes, meta) {
                    metrics.nonlfs.err(1);
                    return Err(e);
                }
                metrics.nonlfs.ok(1);
            }
        }
        self.metrics.write().write += metrics;
        Ok(())
    }

    #[allow(unused_must_use)]
    pub fn flush(&self) -> Result<()> {
        let mut result = Ok(());
        let mut handle_error = |error| {
            tracing::error!(%error);
            result = Err(error);
        };

        if let Some(ref indexedlog_local) = self.indexedlog_local {
            indexedlog_local.flush_log().map_err(&mut handle_error);
        }

        if let Some(ref indexedlog_cache) = self.indexedlog_cache {
            indexedlog_cache.flush_log().map_err(&mut handle_error);
        }

        if let Some(ref lfs_local) = self.lfs_local {
            lfs_local.flush().map_err(&mut handle_error);
        }

        if let Some(ref lfs_cache) = self.lfs_cache {
            lfs_cache.flush().map_err(&mut handle_error);
        }

        if let Some(ref aux_local) = self.aux_local {
            aux_local.flush().map_err(&mut handle_error);
        }

        if let Some(ref aux_cache) = self.aux_cache {
            aux_cache.flush().map_err(&mut handle_error);
        }

        result
    }

    pub fn refresh(&self) -> Result<()> {
        self.metrics.write().api.hg_refresh.call(0);
        if let Some(contentstore) = self.contentstore.as_ref() {
            contentstore.refresh()?;
        }
        self.flush()
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
            activity_logger: None,

            aux_local: None,
            aux_cache: None,

            creation_time: Instant::now(),
            lfs_progress: AggregatingProgressBar::new("fetching", "LFS"),
            flush_on_drop: true,
        }
    }
}

fn use_memcache(creation_time: Instant) -> bool {
    // Only use memcache if the process has been around a while. It takes 2s to setup, which
    // hurts responiveness for short commands.
    creation_time.elapsed() > MEMCACHE_DELAY
}

impl LegacyStore for FileStore {
    /// Returns only the local cache / shared stores, in place of the local-only stores, such that writes will go directly to the local cache.
    /// For compatibility with ContentStore::get_shared_mutable
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
            activity_logger: self.activity_logger.clone(),

            aux_local: None,
            aux_cache: None,

            creation_time: Instant::now(),
            lfs_progress: self.lfs_progress.clone(),

            // Conservatively flushing on drop here, didn't see perf problems and might be needed by Python
            flush_on_drop: true,
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

    fn get_file_content(&self, key: &Key) -> Result<Option<Bytes>> {
        self.metrics.write().api.hg_getfilecontent.call(0);
        self.fetch(
            std::iter::once(key.clone()),
            FileAttributes::CONTENT,
            FetchMode::AllowRemote,
        )
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
                    FetchMode::AllowRemote,
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
                    FetchMode::AllowRemote,
                )
                .single()?
            {
                Some(entry) => StoreResult::Found(entry.content.unwrap().metadata()?),
                None => StoreResult::NotFound(key),
            },
        )
    }

    fn refresh(&self) -> Result<()> {
        self.refresh()
    }
}

impl RemoteDataStore for FileStore {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.metrics.write().api.hg_prefetch.call(keys.len());
        let missing = self
            .fetch(
                keys.iter().cloned().filter_map(|sk| sk.maybe_into_key()),
                FileAttributes::CONTENT,
                FetchMode::AllowRemote,
            )
            .missing()?
            .into_iter()
            .map(StoreKey::HgId)
            .collect();
        Ok(missing)
    }

    fn upload(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.metrics.write().api.hg_upload.call(keys.len());
        // TODO(meyer): Eliminate usage of legacy API, or at least minimize it (do we really need memcache + multiplex, etc)
        if let Some(ref lfs_remote) = self.lfs_remote {
            let mut multiplex = MultiplexDeltaStore::new();
            multiplex.add_store(self.get_shared_mutable());
            if use_memcache(self.creation_time) {
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
            .fetch(
                keys.iter().cloned().filter_map(|sk| sk.maybe_into_key()),
                FileAttributes::CONTENT,
                FetchMode::LocalOnly,
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
                    FetchMode::LocalOnly,
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
                    FetchMode::LocalOnly,
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
