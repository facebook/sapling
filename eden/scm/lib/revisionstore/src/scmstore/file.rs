/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

mod fetch;
mod metrics;
mod types;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use ::metrics::Counter;
use ::types::CasDigest;
use ::types::FetchContext;
use ::types::HgId;
use ::types::Key;
use ::types::fetch_cause::FetchCause;
use ::types::fetch_mode::FetchMode;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::ensure;
use blob::Blob;
use cas_client::CasClient;
use flume::bounded;
use flume::unbounded;
use indexedlog::log::AUTO_SYNC_COUNT;
use indexedlog::log::SYNC_COUNT;
use indexedlog::rotate::ROTATE_COUNT;
use metrics::FILE_STORE_FETCH_METRICS;
use minibytes::Bytes;
use parking_lot::Mutex;
use parking_lot::RwLock;
use progress_model::AggregatingProgressBar;
use progress_model::ProgressBar;
use progress_model::Registry;
use rand::Rng;
use storemodel::SerializationFormat;
use tracing::debug;

pub(crate) use self::fetch::FetchState;
pub use self::metrics::FileStoreFetchMetrics;
pub use self::metrics::FileStoreMetrics;
pub use self::metrics::FileStoreWriteMetrics;
pub use self::types::FileAttributes;
pub use self::types::FileAuxData;
pub(crate) use self::types::LazyFile;
pub use self::types::StoreFile;
use crate::ContentMetadata;
use crate::Delta;
use crate::LocalStore;
use crate::Metadata;
use crate::SaplingRemoteApiFileStore;
use crate::StoreKey;
use crate::StoreResult;
use crate::datastore::HgIdDataStore;
use crate::datastore::HgIdMutableDeltaStore;
use crate::indexedlogauxstore::AuxStore;
use crate::indexedlogdatastore::Entry;
use crate::indexedlogdatastore::IndexedLogHgIdDataStore;
use crate::lfs::LfsClient;
use crate::lfs::LfsPointersEntry;
use crate::scmstore::activitylogger::ActivityLogger;
use crate::scmstore::fetch::FetchResults;
use crate::scmstore::metrics::StoreLocation;
use crate::scmstore::util::try_local_content;

#[derive(Clone)]
pub struct FileStore {
    // Config
    // TODO(meyer): Move these to a separate config struct with default impl, etc.
    pub(crate) lfs_threshold_bytes: Option<u64>,
    pub(crate) edenapi_retries: i32,
    /// Allow explicitly writing serialized LFS pointers outside of tests
    pub(crate) allow_write_lfs_ptrs: bool,

    // Top level flag allow disabling all local computation of aux data.
    pub(crate) compute_aux_data: bool,

    // Local-only stores
    pub(crate) indexedlog_local: Option<Arc<IndexedLogHgIdDataStore>>,

    // Local non-lfs cache aka shared store
    pub(crate) indexedlog_cache: Option<Arc<IndexedLogHgIdDataStore>>,

    // LFS client, containing local store, local cache, and remote client.
    pub(crate) lfs_client: Option<LfsClient>,

    // Remote stores
    pub(crate) edenapi: Option<Arc<SaplingRemoteApiFileStore>>,

    // Aux Data Store
    pub(crate) aux_cache: Option<Arc<AuxStore>>,

    pub(crate) cas_client: Option<Arc<dyn CasClient>>,

    // Metrics, statistics, debugging
    pub(crate) activity_logger: Option<Arc<Mutex<ActivityLogger>>>,
    pub(crate) metrics: Arc<RwLock<FileStoreMetrics>>,

    // Don't flush on drop when we're using FileStore in a "disposable" context, like backingstore
    pub flush_on_drop: bool,

    // The serialization format that the store should use
    pub format: SerializationFormat,

    // The threshold for using CAS cache
    pub(crate) cas_cache_threshold_bytes: Option<u64>,

    // This bar "aggregates" across concurrent uses of this FileStore from different
    // threads (so that only a single progress bar shows up to the user).
    pub(crate) progress_bar: Arc<AggregatingProgressBar>,

    // Temporary escape hatch to disable bounding of queue.
    pub(crate) unbounded_queue: bool,

    // Temporary escape hatch to disable streaming of LFS data to caches.
    pub(crate) lfs_buffer_in_memory: bool,
}

impl Drop for FileStore {
    fn drop(&mut self) {
        if self.flush_on_drop {
            let _ = self.flush();
        }
    }
}

static FILESTORE_FLUSH_COUNT: Counter = Counter::new_counter("scmstore.file.flush");
static INDEXEDLOG_SYNC_COUNT: Counter = Counter::new_counter("scmstore.indexedlog.sync");
static INDEXEDLOG_AUTO_SYNC_COUNT: Counter = Counter::new_counter("scmstore.indexedlog.auto_sync");
static INDEXEDLOG_ROTATE_COUNT: Counter = Counter::new_counter("scmstore.indexedlog.rotate");

impl FileStore {
    /// Get the "local content" without going through the heavyweight "fetch" API.
    pub(crate) fn get_local_content_direct(&self, id: &HgId) -> Result<Option<Blob>> {
        let m = &FILE_STORE_FETCH_METRICS;

        if let Ok(Some(blob)) = self.get_local_content_cas_cache(id) {
            return Ok(Some(blob));
        }
        try_local_content!(id, self.indexedlog_cache, m.indexedlog.cache);
        try_local_content!(id, self.indexedlog_local, m.indexedlog.local);
        try_local_content!(id, self.lfs_client.as_ref().map(|c| &c.shared), m.lfs.cache);
        try_local_content!(
            id,
            self.lfs_client.as_ref().and_then(|c| c.local.as_ref()),
            m.lfs.local
        );
        Ok(None)
    }

    fn get_local_content_cas_cache(&self, id: &HgId) -> Result<Option<Blob>> {
        if let (Some(aux_cache), Some(cas_client)) = (&self.aux_cache, &self.cas_client) {
            let aux_data = aux_cache.get(id)?;
            if let Some(aux_data) = aux_data {
                if let Some(cas_threshold) = self.cas_cache_threshold_bytes {
                    if aux_data.total_size > cas_threshold {
                        // If the file's size exceeds the configured threshold, don't fetch it from CAS.
                        return Ok(None);
                    }
                }

                let (stats, maybe_blob) = cas_client.fetch_single_locally_cached(&CasDigest {
                    hash: aux_data.blake3,
                    size: aux_data.total_size,
                })?;

                FILE_STORE_FETCH_METRICS.cas.fetch(1);
                FILE_STORE_FETCH_METRICS
                    .cas_direct_local_cache
                    .update(&stats);

                if let Some(blob) = maybe_blob {
                    FILE_STORE_FETCH_METRICS.cas.hit(1);
                    return Ok(Some(blob));
                }
            }
        }
        Ok(None)
    }

    pub(crate) fn get_local_aux_direct(&self, id: &HgId) -> Result<Option<FileAuxData>> {
        let m = FILE_STORE_FETCH_METRICS.aux.cache;
        if let Some(store) = &self.aux_cache {
            m.requests.increment();
            m.keys.increment();
            m.singles.increment();
            match store.get(id) {
                Ok(None) => {
                    m.misses.increment();
                }
                Ok(Some(data)) => {
                    m.hits.increment();
                    return Ok(Some(data));
                }
                Err(err) => {
                    m.errors.increment();
                    return Err(err);
                }
            }
        }

        if self.compute_aux_data {
            if let Some(content) = self.get_local_content_direct(id)? {
                m.computed.increment();
                return Ok(Some(FileAuxData::from_content(&content)));
            }
        }

        Ok(None)
    }

    pub fn fetch(
        &self,
        fctx: FetchContext,
        keys: impl IntoIterator<Item = Key>,
        attrs: FileAttributes,
    ) -> FetchResults<StoreFile> {
        let mut keys = keys.into_iter().peekable();
        if keys.peek().is_none() {
            return FetchResults::new(Box::new(std::iter::empty()));
        }

        // Unscientifically picked to be small enough to not use "all" the memory with a
        // full queue of files of decent size, but still generous enough to keep the
        // pipeline full of work for downstream consumers. The important thing is it is
        // less than infinity.
        const RESULT_QUEUE_SIZE: usize = 10_000;

        let bar = self.progress_bar.create_or_extend_local(0);

        let (found_tx, found_rx) = if self.unbounded_queue {
            // Escape hatch in case something goes wrong with bounding.
            unbounded()
        } else {
            // Bound channel size so we don't use unlimited memory queueing up file content
            // when the consumer is consuming slower than we are fetching.
            bounded(RESULT_QUEUE_SIZE)
        };

        let indexedlog_cache = self.indexedlog_cache.clone();

        let mut state = FetchState::new(
            keys,
            attrs,
            self,
            found_tx,
            self.lfs_threshold_bytes.is_some(),
            fctx.clone(),
            self.cas_cache_threshold_bytes,
            bar.clone(),
            indexedlog_cache.clone(),
        );

        // When ignoring results, we won't advance the progress bar, so update the "total".
        if !fctx.mode().ignore_result() {
            bar.increase_total(state.pending_len() as u64);
        }

        if tracing::enabled!(target: "file_fetches", tracing::Level::TRACE) {
            let attrs = [
                attrs.pure_content.then_some("content"),
                attrs.content_header.then_some("header"),
                attrs.aux_data.then_some("aux"),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();

            let mut keys = state.all_keys();
            keys.sort();
            let keys: Vec<_> = keys.into_iter().map(|key| key.path.into_string()).collect();

            tracing::trace!(target: "file_fetches", ?attrs, ?keys);
        }

        debug!(
            ?attrs,
            ?fctx,
            num_keys = state.pending_len(),
            first_keys = "fetching"
        );

        let keys_len = state.pending_len();

        let aux_cache = self.aux_cache.clone();
        let indexedlog_local = self.indexedlog_local.clone();
        let edenapi = self.edenapi.clone();
        let cas_client = self.cas_client.clone();
        let lfs_client = self.lfs_client.clone();
        let activity_logger = self.activity_logger.clone();
        let format = self.format();

        let fetch_local = fctx.mode().contains(FetchMode::LOCAL);
        let fetch_remote = fctx.mode().contains(FetchMode::REMOTE);

        let lfs_buffer_in_memory = self.lfs_buffer_in_memory;

        let process_func = move || {
            // Set bar as this thread's active bar. We don't do it when we create the bar
            // since we might be in a different thread now.
            let _bar = ProgressBar::push_active(bar, Registry::main());

            let start_instant = Instant::now();

            // Only copy keys for activity logger if we have an activity logger;
            let activity_logger_keys: Vec<Key> = if activity_logger.is_some() {
                state.all_keys()
            } else {
                Vec::new()
            };

            let span = tracing::span!(
                tracing::Level::DEBUG,
                "file fetch",
                id = rand::thread_rng().r#gen::<u16>()
            );
            let _enter = span.enter();

            let fetch_from_cas = fetch_remote && cas_client.is_some();

            let mut prev_pending = state.pending_len();
            let mut fetched_since_last_time = |state: &FetchState| -> u64 {
                let new_pending = state.pending_len();
                let diff = prev_pending.saturating_sub(new_pending);
                prev_pending = new_pending;
                diff as u64
            };

            if fetch_local || fetch_from_cas {
                if let Some(ref aux_cache) = aux_cache {
                    state.fetch_aux_indexedlog(
                        aux_cache,
                        StoreLocation::Cache,
                        cas_client.is_some(),
                    );
                }
            }

            if fetch_from_cas {
                // When fetching from CAS, first fetch from local repo to avoid network
                // request for data that is only available locally (e.g. locally
                // committed).
                if fetch_local {
                    if let Some(ref indexedlog_local) = indexedlog_local {
                        state.fetch_indexedlog(indexedlog_local, StoreLocation::Local);
                    }

                    if let Some(lfs_local) = lfs_client.as_ref().and_then(|c| c.local.as_ref()) {
                        state.fetch_lfs(lfs_local, StoreLocation::Local);
                    }
                }

                fctx.inc_local(fetched_since_last_time(&state));

                // Then fetch from CAS since we essentially always expect a hit.
                if let (Some(cas_client), true) = (&cas_client, fetch_remote) {
                    state.fetch_cas(cas_client);
                }

                fctx.inc_remote(fetched_since_last_time(&state));

                // Finally fetch from local cache (shouldn't normally get here).
                if fetch_local {
                    if let Some(ref indexedlog_cache) = indexedlog_cache {
                        state.fetch_indexedlog(indexedlog_cache, StoreLocation::Cache);
                    }

                    if let Some(lfs_cache) = lfs_client.as_ref().map(|c| c.shared.as_ref()) {
                        state.fetch_lfs(lfs_cache, StoreLocation::Cache);
                    }
                }

                fctx.inc_local(fetched_since_last_time(&state));
            } else if fetch_local {
                // If not using CAS, fetch from cache first then local (hit rate in cache
                // is typically much higher).
                if let Some(ref indexedlog_cache) = indexedlog_cache {
                    state.fetch_indexedlog(indexedlog_cache, StoreLocation::Cache);
                }

                if let Some(ref indexedlog_local) = indexedlog_local {
                    state.fetch_indexedlog(indexedlog_local, StoreLocation::Local);
                }

                fctx.inc_local(fetched_since_last_time(&state));

                if let Some(lfs_cache) = lfs_client.as_ref().map(|c| c.shared.as_ref()) {
                    assert!(
                        format == SerializationFormat::Hg,
                        "LFS cannot be used with non-Hg serialization format"
                    );
                    state.fetch_lfs(lfs_cache, StoreLocation::Cache);
                }

                if let Some(lfs_local) = lfs_client.as_ref().and_then(|c| c.local.as_ref()) {
                    assert!(
                        format == SerializationFormat::Hg,
                        "LFS cannot be used with non-Hg serialization format"
                    );
                    state.fetch_lfs(lfs_local, StoreLocation::Local);
                }

                fctx.inc_local(fetched_since_last_time(&state));
            }

            if fetch_remote {
                if let Some(ref edenapi) = edenapi {
                    state.fetch_edenapi(
                        edenapi,
                        indexedlog_cache.clone(),
                        lfs_client.as_ref().map(|c| c.shared.clone()),
                        aux_cache.clone(),
                    );
                }

                if let Some(ref lfs_client) = lfs_client {
                    assert!(
                        format == SerializationFormat::Hg,
                        "LFS cannot be used with non-Hg serialization format"
                    );
                    state.fetch_lfs_remote(lfs_client, lfs_buffer_in_memory);
                }

                fctx.inc_remote(fetched_since_last_time(&state));
            }

            state.derive_computable(aux_cache.as_ref().map(|s| s.as_ref()));

            state.finish();

            // These aren't technically filestore specific, but this will keep them updated.
            INDEXEDLOG_SYNC_COUNT.add(SYNC_COUNT.swap(0, Ordering::Relaxed) as usize);
            INDEXEDLOG_AUTO_SYNC_COUNT.add(AUTO_SYNC_COUNT.swap(0, Ordering::Relaxed) as usize);
            INDEXEDLOG_ROTATE_COUNT.add(ROTATE_COUNT.swap(0, Ordering::Relaxed) as usize);

            if let Some(activity_logger) = activity_logger {
                if let Err(err) = activity_logger.lock().log_file_fetch(
                    activity_logger_keys,
                    attrs,
                    start_instant.elapsed(),
                ) {
                    tracing::error!("Error writing activity log: {}", err);
                }
            }
        };

        // Only kick off a thread if there's a substantial amount of work.
        //
        // NB: callers such as backingstore::prefetch assume asynchronous behavior when fetching
        // more than 1k keys. If you change how this works, consider callers' expectations
        // carefully.
        if keys_len > 1000 {
            let active_bar = Registry::main().get_active_progress_bar();
            std::thread::spawn(move || {
                // Propagate parent progress bar into the thread so things nest well.
                Registry::main().set_active_progress_bar(active_bar);
                process_func();
            });
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
        ensure!(
            self.format() == SerializationFormat::Hg,
            "LFS cannot be used with non-Hg serialization format"
        );
        let lfs_local = self
            .lfs_client
            .as_ref()
            .and_then(|c| c.local.as_ref())
            .ok_or_else(|| {
                anyhow!("trying to write LFS pointer but no local LfsStore is available")
            })?;

        let lfs_pointer = LfsPointersEntry::from_bytes(bytes, key.hgid)?;
        lfs_local.add_pointer(lfs_pointer)
    }

    fn write_lfs(&self, key: Key, bytes: Bytes) -> Result<()> {
        let lfs_local = self
            .lfs_client
            .as_ref()
            .and_then(|c| c.local.as_ref())
            .ok_or_else(|| {
                anyhow!("trying to write LFS file but no local LfsStore is available")
            })?;
        ensure!(
            self.format() == SerializationFormat::Hg,
            "LFS cannot be used with non-Hg serialization format"
        );

        lfs_local.add_blob_and_pointer(key, bytes)?;

        Ok(())
    }

    pub(crate) fn write_nonlfs(&self, key: Key, bytes: Bytes, meta: Metadata) -> Result<()> {
        let indexedlog_local = self.indexedlog_local.as_ref().ok_or_else(|| {
            anyhow!("trying to write non-LFS file but no local non-LFS IndexedLog is available")
        })?;
        indexedlog_local.put_entry(Entry::new(key.hgid, bytes, meta))?;

        Ok(())
    }

    pub fn write_batch(&self, entries: impl Iterator<Item = (Key, Bytes, Metadata)>) -> Result<()> {
        // TODO(meyer): Don't fail the whole batch for a single write error.
        let mut metrics = FileStoreWriteMetrics::default();
        for (key, bytes, meta) in entries {
            if meta.is_lfs() && self.lfs_threshold_bytes.is_some() {
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
                .is_some_and(|threshold| hg_blob_len > threshold)
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
    #[tracing::instrument(level = "debug", skip(self))]
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

        if let Some(lfs_client) = &self.lfs_client {
            lfs_client.flush().map_err(&mut handle_error);
        }

        if let Some(ref aux_cache) = self.aux_cache {
            aux_cache.flush().map_err(&mut handle_error);
        }

        let metrics = std::mem::take(&mut *self.metrics.write());
        for (k, v) in metrics.metrics() {
            hg_metrics::increment_counter(k, v as u64);
        }

        FILESTORE_FLUSH_COUNT.increment();

        result
    }

    pub fn refresh(&self) -> Result<()> {
        self.metrics.write().api.hg_refresh.call(0);
        self.flush()
    }

    pub fn metrics(&self) -> Vec<(String, usize)> {
        self.metrics.read().metrics().collect()
    }

    pub fn empty() -> Self {
        FileStore {
            lfs_threshold_bytes: None,
            edenapi_retries: 0,
            allow_write_lfs_ptrs: false,

            compute_aux_data: false,

            indexedlog_local: None,

            indexedlog_cache: None,

            edenapi: None,
            lfs_client: None,
            cas_client: None,

            metrics: FileStoreMetrics::new(),
            activity_logger: None,

            aux_cache: None,

            flush_on_drop: true,
            format: SerializationFormat::Hg,

            cas_cache_threshold_bytes: None,

            progress_bar: AggregatingProgressBar::new("", ""),

            unbounded_queue: false,

            lfs_buffer_in_memory: false,
        }
    }

    pub fn indexedlog_local(&self) -> Option<Arc<IndexedLogHgIdDataStore>> {
        self.indexedlog_local.clone()
    }

    pub fn indexedlog_cache(&self) -> Option<Arc<IndexedLogHgIdDataStore>> {
        self.indexedlog_cache.clone()
    }

    /// Returns only the local cache / shared stores, in place of the local-only stores,
    /// such that writes will go directly to the local cache.
    pub fn with_shared_only(&self) -> Self {
        // this is infallible in ContentStore so panic if there are no shared/cache stores.
        assert!(
            self.indexedlog_cache.is_some() || self.lfs_client.is_some(),
            "cannot get shared_mutable, no shared / local cache stores available"
        );

        Self {
            lfs_threshold_bytes: self.lfs_threshold_bytes.clone(),
            edenapi_retries: self.edenapi_retries.clone(),
            allow_write_lfs_ptrs: self.allow_write_lfs_ptrs,

            compute_aux_data: self.compute_aux_data,

            indexedlog_local: self.indexedlog_cache.clone(),

            indexedlog_cache: None,

            edenapi: None,
            lfs_client: self.lfs_client.as_ref().map(|c| c.with_shared_only()),
            cas_client: None,

            metrics: self.metrics.clone(),
            activity_logger: self.activity_logger.clone(),

            aux_cache: None,

            // Conservatively flushing on drop here, didn't see perf problems and might be needed by Python
            flush_on_drop: true,
            format: self.format(),

            cas_cache_threshold_bytes: self.cas_cache_threshold_bytes.clone(),

            progress_bar: self.progress_bar.clone(),

            unbounded_queue: self.unbounded_queue,

            lfs_buffer_in_memory: self.lfs_buffer_in_memory,
        }
    }

    // Returns keys that weren't found locally.
    pub fn upload_lfs(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.metrics.write().api.hg_upload.call(keys.len());
        if let Some(ref lfs_client) = self.lfs_client {
            lfs_client.upload(keys)
        } else {
            Ok(keys.to_vec())
        }
    }

    pub fn format(&self) -> SerializationFormat {
        self.format
    }

    pub fn prefetch(&self, keys: Vec<Key>) -> Result<Vec<Key>> {
        self.metrics.write().api.hg_prefetch.call(keys.len());

        self.fetch(
            FetchContext::new_with_cause(
                FetchMode::AllowRemote | FetchMode::IGNORE_RESULT,
                FetchCause::SaplingPrefetch,
            ),
            keys,
            FileAttributes::CONTENT,
        )
        .missing()
    }

    pub fn metadata(&self, key: StoreKey) -> Result<StoreResult<ContentMetadata>> {
        self.metrics.write().api.contentdatastore_metadata.call(0);

        if let Some(cache) = self.lfs_client.as_ref().map(|c| &c.shared) {
            let result = cache.metadata(key.clone())?;
            if matches!(result, StoreResult::Found(_)) {
                return Ok(result);
            }
        }

        if let Some(local) = self.lfs_client.as_ref().and_then(|c| c.local.as_ref()) {
            let result = local.metadata(key.clone())?;
            if matches!(result, StoreResult::Found(_)) {
                return Ok(result);
            }
        }

        Ok(StoreResult::NotFound(key))
    }
}

impl HgIdDataStore for FileStore {
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        self.metrics.write().api.hg_get.call(0);
        Ok(
            match self
                .fetch(
                    FetchContext::default(),
                    std::iter::once(key.clone()).filter_map(|sk| sk.maybe_into_key()),
                    FileAttributes::CONTENT,
                )
                .single()?
            {
                Some(entry) => StoreResult::Found(entry.hg_content()?.into_vec()),
                None => StoreResult::NotFound(key),
            },
        )
    }

    fn refresh(&self) -> Result<()> {
        self.refresh()
    }
}

impl LocalStore for FileStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.metrics.write().api.hg_getmissing.call(keys.len());
        Ok(self
            .fetch(
                FetchContext::new(FetchMode::LocalOnly | FetchMode::IGNORE_RESULT),
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
