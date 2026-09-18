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
use std::time::Instant;

use ::metrics::Counter;
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
use cas_client::CasFetchManager;
use metrics::FILE_STORE_FETCH_METRICS;
use minibytes::Bytes;
use parking_lot::Mutex;
use parking_lot::RwLock;
use progress_model::AggregatingProgressBar;
use progress_model::ProgressBar;
use progress_model::Registry;
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
use crate::scmstore::fetch::MaxFetchCount;
use crate::scmstore::metrics::StoreLocation;
use crate::scmstore::util::try_local_content;

#[derive(Clone)]
pub struct FileStore {
    // Config
    // TODO(meyer): Move these to a separate config struct with default impl, etc.
    pub(crate) lfs_threshold_bytes: Option<u64>,
    pub(crate) edenapi_retries: i32,
    pub(crate) verify_hash: bool,
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
    pub(crate) cas_manager: Option<Arc<CasFetchManager>>,

    // Aux Data Store
    pub(crate) aux_cache: Option<Arc<AuxStore>>,

    // Metrics, statistics, debugging
    pub(crate) activity_logger: Option<Arc<Mutex<ActivityLogger>>>,
    pub(crate) metrics: Arc<RwLock<FileStoreMetrics>>,

    // Don't flush on drop when we're using FileStore in a "disposable" context, like backingstore
    pub flush_on_drop: bool,

    // The serialization format that the store should use
    pub format: SerializationFormat,

    // This bar "aggregates" across concurrent uses of this FileStore from different
    // threads (so that only a single progress bar shows up to the user).
    pub(crate) progress_bar: Arc<AggregatingProgressBar>,

    // Temporary escape hatch to disable streaming of LFS data to caches.
    pub(crate) lfs_buffer_in_memory: bool,

    // Bounds the number of items this store can deliver across the lifetime of
    // the process. When exceeded, every subsequent item becomes an error,
    // catching all callers and code paths (including serial fetches). Set via
    // `FileStoreBuilder::max_fetch_count`; absent means the guard is disabled.
    pub(crate) max_fetch_count: MaxFetchCount,
}

impl Drop for FileStore {
    fn drop(&mut self) {
        if self.flush_on_drop {
            let _ = self.flush();
        }
    }
}

static FILESTORE_FLUSH_COUNT: Counter = Counter::new_counter("scmstore.file.flush");

impl FileStore {
    /// Get the "local content" without going through the heavyweight "fetch" API.
    pub(crate) fn get_local_content_direct(&self, id: &HgId) -> Result<Option<Blob>> {
        let m = &FILE_STORE_FETCH_METRICS;

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
        let keys: Vec<_> = keys.into_iter().collect();
        if keys.is_empty() {
            return FetchResults::empty();
        }

        let bar = self.progress_bar.create_or_extend_local(0);

        let indexedlog_cache = self.indexedlog_cache.clone();
        let keys_len = keys.len();

        let aux_cache = self.aux_cache.clone();
        let indexedlog_local = self.indexedlog_local.clone();
        let edenapi = self.edenapi.clone();
        let cas_manager = self.cas_manager.clone();
        let lfs_client = self.lfs_client.clone();
        let activity_logger = self.activity_logger.clone();
        let format = self.format();

        let fetch_local = fctx.mode().contains(FetchMode::LOCAL);
        let fetch_remote = fctx.mode().contains(FetchMode::REMOTE);
        let sync_mode = fctx.sync_mode();

        let lfs_buffer_in_memory = self.lfs_buffer_in_memory;
        let lfs_enabled = self.lfs_threshold_bytes.is_some();
        let verify_hash = self.verify_hash;
        let compute_aux_data = self.compute_aux_data;
        let max_fetch_count = self.max_fetch_count.clone();

        FetchResults::from_process(sync_mode.should_spawn(keys_len), move |results| {
            let mut state = FetchState::new(
                keys,
                attrs,
                results,
                compute_aux_data,
                lfs_enabled,
                verify_hash,
                fctx.clone(),
                bar.clone(),
                format,
                indexedlog_cache.clone(),
                max_fetch_count,
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

                let mut keys = state.unique_keys();
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

            // Set bar as this thread's active bar. We don't do it when we create the bar
            // since we might be in a different thread now.
            let _bar = ProgressBar::push_active(bar, Registry::main());

            let start_instant = Instant::now();

            // Only copy keys for activity logger if we have an activity logger;
            let activity_logger_keys: Vec<Key> = if activity_logger.is_some() {
                state.unique_keys()
            } else {
                Vec::new()
            };

            let span = tracing::span!(
                tracing::Level::DEBUG,
                "file fetch",
                id = rand::random::<u16>()
            );
            let _enter = span.enter();

            let fetch_from_cas = fetch_remote && cas_manager.is_some();

            let mut prev_pending = state.pending_len();
            let mut fetched_since_last_time = |state: &FetchState| -> u64 {
                let new_pending = state.pending_len();
                let diff = prev_pending.saturating_sub(new_pending);
                prev_pending = new_pending;
                diff as u64
            };

            if fetch_local || fetch_from_cas {
                if let Some(ref aux_cache) = aux_cache {
                    state.fetch_aux_indexedlog(aux_cache, StoreLocation::Cache, fetch_from_cas);
                }
            }

            if fetch_local {
                // Fetch from cache first then local (hit rate in cache is typically much higher).
                if let Some(ref indexedlog_cache) = indexedlog_cache {
                    state.fetch_indexedlog(indexedlog_cache, StoreLocation::Cache);
                }

                if let Some(ref indexedlog_local) = indexedlog_local {
                    state.fetch_indexedlog(indexedlog_local, StoreLocation::Local);
                }

                fctx.inc_local(fetched_since_last_time(&state));

                if !fctx.skip_lfs() {
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
                }

                fctx.inc_local(fetched_since_last_time(&state));
            }

            if fetch_remote {
                if let Some(ref cas_manager) = cas_manager {
                    state.fetch_cas(cas_manager.as_ref());
                }

                if let Some(ref edenapi) = edenapi {
                    state.fetch_edenapi(
                        edenapi,
                        indexedlog_cache.clone(),
                        lfs_client.as_ref().map(|c| c.shared.clone()),
                        aux_cache.clone(),
                    );
                }

                if !fctx.skip_lfs() {
                    if let Some(ref lfs_client) = lfs_client {
                        assert!(
                            format == SerializationFormat::Hg,
                            "LFS cannot be used with non-Hg serialization format"
                        );
                        state.fetch_lfs_remote(lfs_client, lfs_buffer_in_memory);
                    }
                }

                fctx.inc_remote(fetched_since_last_time(&state));
            }

            state.derive_computable(aux_cache.as_ref().map(|s| s.as_ref()));

            state.finish();

            if let Some(activity_logger) = activity_logger {
                if let Err(err) = activity_logger.lock().log_file_fetch(
                    activity_logger_keys,
                    attrs,
                    start_instant.elapsed(),
                ) {
                    tracing::error!("Error writing activity log: {}", err);
                }
            }
        })
    }

    pub(crate) fn write_lfsptr(&self, key: Key, bytes: Bytes) -> Result<()> {
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

    pub(crate) fn write_lfs(&self, key: Key, bytes: Bytes) -> Result<()> {
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
        self.flush_inner(true)
    }

    pub fn sync(&self) -> Result<()> {
        self.metrics.write().api.hg_refresh.call(0);
        self.flush_inner(false)
    }

    fn flush_inner(&self, skip_clean: bool) -> Result<()> {
        let mut result = Ok(());
        let mut handle_error = |error| {
            tracing::error!(%error);
            result = Err(error);
        };

        if let Some(ref indexedlog_local) = self.indexedlog_local {
            if !skip_clean || indexedlog_local.is_dirty() {
                indexedlog_local.flush_log().map_err(&mut handle_error).ok();
            }
        }

        if let Some(ref indexedlog_cache) = self.indexedlog_cache {
            if !skip_clean || indexedlog_cache.is_dirty() {
                indexedlog_cache.flush_log().map_err(&mut handle_error).ok();
            }
        }

        if let Some(lfs_client) = &self.lfs_client {
            if !skip_clean || lfs_client.is_dirty() {
                lfs_client.flush().map_err(&mut handle_error).ok();
            }
        }

        if let Some(ref aux_cache) = self.aux_cache {
            if !skip_clean || aux_cache.is_dirty() {
                aux_cache.flush().map_err(&mut handle_error).ok();
            }
        }

        let metrics = std::mem::take(&mut *self.metrics.write());
        for (k, v) in metrics.metrics() {
            hg_metrics::increment_counter(k, v as u64);
        }

        FILESTORE_FLUSH_COUNT.increment();

        result
    }

    pub fn metrics(&self) -> Vec<(String, usize)> {
        self.metrics.read().metrics().collect()
    }

    pub fn empty() -> Self {
        FileStore {
            lfs_threshold_bytes: None,
            verify_hash: true,
            edenapi_retries: 0,
            allow_write_lfs_ptrs: false,

            compute_aux_data: false,

            indexedlog_local: None,

            indexedlog_cache: None,

            edenapi: None,
            cas_manager: None,
            lfs_client: None,

            metrics: FileStoreMetrics::new(),
            activity_logger: None,

            aux_cache: None,

            flush_on_drop: true,
            format: SerializationFormat::Hg,

            progress_bar: AggregatingProgressBar::new("", ""),

            lfs_buffer_in_memory: false,

            max_fetch_count: Default::default(),
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
            verify_hash: self.verify_hash,
            edenapi_retries: self.edenapi_retries.clone(),
            allow_write_lfs_ptrs: self.allow_write_lfs_ptrs,

            compute_aux_data: self.compute_aux_data,

            indexedlog_local: self.indexedlog_cache.clone(),

            indexedlog_cache: None,

            edenapi: None,
            cas_manager: None,
            lfs_client: self.lfs_client.as_ref().map(|c| c.with_shared_only()),

            metrics: self.metrics.clone(),
            activity_logger: self.activity_logger.clone(),

            aux_cache: None,

            // Conservatively flushing on drop here, didn't see perf problems and might be needed by Python
            flush_on_drop: true,
            format: self.format(),

            progress_bar: self.progress_bar.clone(),

            lfs_buffer_in_memory: self.lfs_buffer_in_memory,

            max_fetch_count: self.max_fetch_count.clone(),
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
            FetchContext::new_with_mode_and_cause(
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

    fn sync(&self) -> Result<()> {
        self.sync()
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use ::types::RepoPathBuf;
    use ::types::fetch_cause::FetchCause;
    use ::types::fetch_mode::FetchMode;
    use async_runtime::block_on;
    use cas_client::CasBatch;
    use cas_client::CasClient;
    use cas_client::CasDigest;
    use cas_client::CasDigestType;
    use futures::StreamExt;
    use futures::stream;
    use futures::stream::BoxStream;
    use storemodel::InsertOpts;
    use storemodel::Kind;
    use tempfile::TempDir;

    use super::*;
    use crate::StoreType;
    use crate::indexedlogdatastore::IndexedLogHgIdDataStoreConfig;

    fn make_indexedlog(tempdir: &TempDir) -> Arc<IndexedLogHgIdDataStore> {
        let config = IndexedLogHgIdDataStoreConfig {
            max_log_count: None,
            max_bytes_per_log: None,
            max_bytes: None,
            btrfs_compression: false,
        };
        Arc::new(
            IndexedLogHgIdDataStore::new(
                &BTreeMap::<&str, &str>::new(),
                tempdir,
                &config,
                StoreType::Rotated,
                SerializationFormat::Hg,
            )
            .unwrap(),
        )
    }

    struct FakeCasClient {
        content: Bytes,
        fetch_count: AtomicUsize,
        failed_digest: Option<CasDigest>,
    }

    impl CasClient for FakeCasClient {
        fn fetch<'a>(
            &'a self,
            digests: &'a [CasDigest],
            _digest_type: CasDigestType,
        ) -> BoxStream<'a, Result<CasBatch>> {
            self.fetch_count.fetch_add(1, Ordering::Relaxed);
            let content = Blob::Bytes(self.content.clone());
            let results = digests
                .iter()
                .copied()
                .map(|digest| {
                    let result = if self.failed_digest == Some(digest) {
                        Err(anyhow!("injected CAS fetch failure"))
                    } else {
                        Ok(Some(content.clone()))
                    };
                    (digest, result)
                })
                .collect();
            let batch = CasBatch {
                backend_stats: Default::default(),
                results,
            };
            stream::once(async move { Ok(batch) }).boxed()
        }
    }

    #[test]
    fn test_skip_lfs_skips_lfs_fetch() {
        let il_dir = TempDir::new().unwrap();
        let indexedlog = make_indexedlog(&il_dir);

        let lfs_key = Key::new(
            RepoPathBuf::from_string("large.bin".to_string()).unwrap(),
            HgId::from_hex(b"2222222222222222222222222222222222222222").unwrap(),
        );
        let content = Bytes::from_static(b"large file content here");

        // Write file content to indexedlog with LFS flag set.
        let lfs_meta = Metadata {
            flags: Some(Metadata::LFS_FLAG),
            size: None,
        };
        indexedlog
            .put_entry(Entry::new(lfs_key.hgid, content.clone(), lfs_meta))
            .unwrap();

        // Set up LFS store with the actual blob.
        let lfs_dir = TempDir::new().unwrap();
        let server = mockito::Server::new();
        let lfs_config = crate::testutil::make_lfs_config(&server, &lfs_dir, "skip_lfs");
        let lfs_store = Arc::new(crate::lfs::LfsStore::rotated(&lfs_dir, &lfs_config).unwrap());
        lfs_store
            .add_blob_and_pointer(lfs_key.clone(), content)
            .unwrap();
        lfs_store.flush().unwrap();
        let lfs_client = crate::lfs::LfsClient::new(lfs_store, None, &lfs_config).unwrap();

        let make_store = || {
            let mut store = FileStore::empty();
            store.indexedlog_local = Some(indexedlog.clone());
            store.lfs_client = Some(lfs_client.clone());
            store.lfs_threshold_bytes = Some(1);
            store
        };

        // Without skip_lfs, the LFS key resolves successfully.
        let fctx = FetchContext::new_with_mode_and_cause(
            FetchMode::LocalOnly,
            FetchCause::EdenWalkPrefetch,
        );
        let results: Vec<_> = make_store()
            .fetch(fctx, vec![lfs_key.clone()], FileAttributes::CONTENT)
            .into_iter()
            .collect();
        assert!(
            results[0].is_ok(),
            "LFS key should resolve without skip_lfs"
        );

        // With skip_lfs, the LFS fetch is skipped so the key is not found.
        let fctx = FetchContext::new_with_mode_and_cause(
            FetchMode::LocalOnly,
            FetchCause::EdenWalkPrefetch,
        )
        .with_skip_lfs(true);
        let results: Vec<_> = make_store()
            .fetch(fctx, vec![lfs_key.clone()], FileAttributes::CONTENT)
            .into_iter()
            .collect();
        assert!(
            results[0].is_err(),
            "LFS key should not be found when skip_lfs=true"
        );
    }

    #[test]
    fn test_blob_origin_remote_then_local() {
        // An hg blob body is `min(p1, p2) + max(p1, p2) + text`; with no
        // parents both hashes are zero.
        let remote_dir = TempDir::new().unwrap();
        let remote_repo = eagerepo::EagerRepo::open(remote_dir.path()).unwrap();
        let text = b"remote-then-local content\n";
        let mut blob = vec![0u8; HgId::len() * 2];
        blob.extend_from_slice(text);
        let hgid = remote_repo.add_sha1_blob(&blob).unwrap();
        block_on(remote_repo.flush()).unwrap();

        let key = Key::new(
            RepoPathBuf::from_string("foo.txt".to_string()).unwrap(),
            hgid,
        );

        let cache_dir = TempDir::new().unwrap();
        let mut store = FileStore::empty();
        store.indexedlog_cache = Some(make_indexedlog(&cache_dir));
        store.edenapi = Some(SaplingRemoteApiFileStore::new(Arc::new(remote_repo)));

        // Fetch 1: cold local cache, AllowRemote. The local cache misses and
        // the blob is served from the fake remote, warming indexedlog_cache
        // via write-through.
        let fctx = FetchContext::new(FetchMode::AllowRemote);
        let results: Vec<_> = store
            .fetch(fctx.clone(), vec![key.clone()], FileAttributes::CONTENT)
            .into_iter()
            .collect();
        let content = results[0]
            .as_ref()
            .expect("blob should be fetched from the remote")
            .1
            .file_content()
            .unwrap();
        assert_eq!(content.into_bytes().as_ref(), text);
        assert_eq!(fctx.remote_fetch_count(), 1);
        assert_eq!(fctx.local_fetch_count(), 0);

        store.flush().unwrap();

        // Fetch 2: repeat the identical AllowRemote request now that the
        // cache is warm. A real prefetch always requests AllowRemote (it
        // doesn't know ahead of time what's cached), so this -- unlike a
        // LocalOnly request -- proves scmstore's local-first check actually
        // skips the remote on a cache hit, not just that an explicit
        // local-only request can be served locally.
        let fctx = FetchContext::new(FetchMode::AllowRemote);
        let results: Vec<_> = store
            .fetch(fctx.clone(), vec![key.clone()], FileAttributes::CONTENT)
            .into_iter()
            .collect();
        let content = results[0]
            .as_ref()
            .expect("blob should be served from the local cache")
            .1
            .file_content()
            .unwrap();
        assert_eq!(content.into_bytes().as_ref(), text);
        assert_eq!(fctx.local_fetch_count(), 1);
        assert_eq!(fctx.remote_fetch_count(), 0);
    }

    #[test]
    fn test_cas_prefetch_writes_through_to_indexedlog() -> Result<()> {
        let content_bytes = Bytes::from_static(b"content from cas\n");
        let content = Blob::Bytes(content_bytes.clone());
        let mut aux_data = FileAuxData::from_content(&content);
        aux_data.file_header_metadata = Some(Bytes::new());
        let key = Key::new(
            RepoPathBuf::from_string("cas.txt".to_string())?,
            HgId::from_hex(b"3333333333333333333333333333333333333333")?,
        );

        let cache_dir = TempDir::new()?;
        let aux_dir = TempDir::new()?;
        let aux_cache = Arc::new(AuxStore::new(
            aux_dir.path(),
            &BTreeMap::<&str, &str>::new(),
            StoreType::Rotated,
        )?);
        aux_cache.put(key.hgid, &aux_data)?;

        let cas_client = Arc::new(FakeCasClient {
            content: content_bytes,
            fetch_count: AtomicUsize::new(0),
            failed_digest: None,
        });
        let mut store = FileStore::empty();
        store.indexedlog_cache = Some(make_indexedlog(&cache_dir));
        store.aux_cache = Some(aux_cache);
        store.cas_manager = Some(Arc::new(
            CasFetchManager::builder(cas_client.clone()).build(),
        ));

        let first_context = FetchContext::new(FetchMode::AllowRemote | FetchMode::IGNORE_RESULT);
        let first: Vec<_> = store
            .fetch(
                first_context.clone(),
                [key.clone()],
                FileAttributes::CONTENT,
            )
            .into_iter()
            .collect();
        assert!(first.is_empty(), "prefetch should not return file content");
        assert!(first_context.fetch_from_cas_attempted());
        assert_eq!(first_context.local_fetch_count(), 0);
        assert_eq!(first_context.remote_fetch_count(), 1);
        assert_eq!(cas_client.fetch_count.load(Ordering::Relaxed), 1);

        let second_context = FetchContext::new(FetchMode::AllowRemote);
        let second = store
            .fetch(second_context.clone(), [key], FileAttributes::CONTENT)
            .single()?
            .expect("hgcache should provide the file");
        assert_eq!(second.file_content()?, content);
        assert_eq!(second_context.local_fetch_count(), 1);
        assert_eq!(second_context.remote_fetch_count(), 0);
        assert_eq!(cas_client.fetch_count.load(Ordering::Relaxed), 1);

        Ok(())
    }

    #[test]
    fn test_cas_error_falls_back_to_edenapi() -> Result<()> {
        let remote_dir = TempDir::new()?;
        let remote_repo = eagerepo::EagerRepo::open(remote_dir.path())?;
        let content_bytes = Bytes::from_static(b"content from edenapi\n");
        let mut hg_blob = vec![0u8; HgId::len() * 2];
        hg_blob.extend_from_slice(&content_bytes);
        let hgid = remote_repo.add_sha1_blob(&hg_blob)?;
        block_on(remote_repo.flush())?;

        let key = Key::new(RepoPathBuf::from_string("fallback.txt".to_string())?, hgid);
        let content = Blob::Bytes(content_bytes.clone());
        let mut aux_data = FileAuxData::from_content(&content);
        aux_data.file_header_metadata = Some(Bytes::new());
        let aux_dir = TempDir::new()?;
        let aux_cache = Arc::new(AuxStore::new(
            aux_dir.path(),
            &BTreeMap::<&str, &str>::new(),
            StoreType::Rotated,
        )?);
        aux_cache.put(key.hgid, &aux_data)?;

        let cas_client = Arc::new(FakeCasClient {
            content: Bytes::new(),
            fetch_count: AtomicUsize::new(0),
            failed_digest: Some(CasDigest {
                hash: aux_data.blake3,
                size: aux_data.total_size,
            }),
        });
        let mut store = FileStore::empty();
        store.aux_cache = Some(aux_cache);
        store.edenapi = Some(SaplingRemoteApiFileStore::new(Arc::new(remote_repo)));
        store.cas_manager = Some(Arc::new(
            CasFetchManager::builder(cas_client.clone()).build(),
        ));

        let context = FetchContext::new(FetchMode::AllowRemote);
        let fetched = store
            .fetch(context.clone(), [key], FileAttributes::CONTENT)
            .single()?
            .expect("EdenAPI should satisfy the failed CAS request");

        assert_eq!(fetched.file_content()?, content);
        assert!(context.fetch_from_cas_attempted());
        assert_eq!(cas_client.fetch_count.load(Ordering::Relaxed), 1);

        Ok(())
    }

    #[test]
    fn test_partial_cas_file_failure_falls_back_to_edenapi() -> Result<()> {
        let remote_dir = TempDir::new()?;
        let remote_repo = eagerepo::EagerRepo::open(remote_dir.path())?;

        let cas_content = Bytes::from_static(b"content from cas\n");
        let cas_hgid = crate::trait_impls::sha1_digest(
            &InsertOpts {
                kind: Kind::File,
                ..Default::default()
            },
            &cas_content,
            SerializationFormat::Hg,
        );
        let cas_key = Key::new(RepoPathBuf::from_string("cas.txt".to_string())?, cas_hgid);
        let mut cas_aux = FileAuxData::from_content(&Blob::Bytes(cas_content.clone()));
        cas_aux.file_header_metadata = Some(Bytes::new());

        let fallback_content = Bytes::from_static(b"content from edenapi\n");
        let mut fallback_hg_blob = vec![0u8; HgId::len() * 2];
        fallback_hg_blob.extend_from_slice(&fallback_content);
        let fallback_hgid = remote_repo.add_sha1_blob(&fallback_hg_blob)?;
        block_on(remote_repo.flush())?;
        let fallback_key = Key::new(
            RepoPathBuf::from_string("fallback.txt".to_string())?,
            fallback_hgid,
        );
        let mut fallback_aux = FileAuxData::from_content(&Blob::Bytes(fallback_content.clone()));
        fallback_aux.file_header_metadata = Some(Bytes::new());
        let failed_digest = CasDigest {
            hash: fallback_aux.blake3,
            size: fallback_aux.total_size,
        };

        let aux_dir = TempDir::new()?;
        let aux_cache = Arc::new(AuxStore::new(
            aux_dir.path(),
            &BTreeMap::<&str, &str>::new(),
            StoreType::Rotated,
        )?);
        aux_cache.put(cas_key.hgid, &cas_aux)?;
        aux_cache.put(fallback_key.hgid, &fallback_aux)?;

        let cas_client = Arc::new(FakeCasClient {
            content: cas_content.clone(),
            fetch_count: AtomicUsize::new(0),
            failed_digest: Some(failed_digest),
        });
        let mut store = FileStore::empty();
        store.aux_cache = Some(aux_cache);
        store.edenapi = Some(SaplingRemoteApiFileStore::new(Arc::new(remote_repo)));
        store.cas_manager = Some(Arc::new(
            CasFetchManager::builder(cas_client.clone()).build(),
        ));

        let context = FetchContext::new(FetchMode::AllowRemote);
        let fetched = store
            .fetch(
                context.clone(),
                [cas_key.clone(), fallback_key.clone()],
                FileAttributes::CONTENT,
            )
            .into_iter()
            .collect::<std::result::Result<Vec<_>, _>>()?;

        assert_eq!(fetched.len(), 2, "both files should be fetched");
        let cas_file = fetched
            .iter()
            .find(|(key, _)| key == &cas_key)
            .expect("CAS result should be present");
        assert_eq!(cas_file.1.file_content()?, Blob::Bytes(cas_content));
        let fallback_file = fetched
            .iter()
            .find(|(key, _)| key == &fallback_key)
            .expect("EdenAPI fallback result should be present");
        assert_eq!(
            fallback_file.1.file_content()?,
            Blob::Bytes(fallback_content)
        );
        assert!(context.fetch_from_cas_attempted());
        assert_eq!(cas_client.fetch_count.load(Ordering::Relaxed), 1);

        Ok(())
    }

    #[test]
    fn test_cas_skips_file_without_content_header() -> Result<()> {
        let remote_dir = TempDir::new()?;
        let remote_repo = eagerepo::EagerRepo::open(remote_dir.path())?;
        let content_bytes = Bytes::from_static(b"content from edenapi\n");
        let mut hg_blob = vec![0u8; HgId::len() * 2];
        hg_blob.extend_from_slice(&content_bytes);
        let hgid = remote_repo.add_sha1_blob(&hg_blob)?;
        block_on(remote_repo.flush())?;

        let key = Key::new(
            RepoPathBuf::from_string("missing-header.txt".to_string())?,
            hgid,
        );
        let aux_data = FileAuxData::from_content(&Blob::Bytes(content_bytes.clone()));
        let aux_dir = TempDir::new()?;
        let aux_cache = Arc::new(AuxStore::new(
            aux_dir.path(),
            &BTreeMap::<&str, &str>::new(),
            StoreType::Rotated,
        )?);
        aux_cache.put(key.hgid, &aux_data)?;

        let cas_client = Arc::new(FakeCasClient {
            content: content_bytes.clone(),
            fetch_count: AtomicUsize::new(0),
            failed_digest: None,
        });
        let cache_dir = TempDir::new()?;
        let mut store = FileStore::empty();
        store.indexedlog_cache = Some(make_indexedlog(&cache_dir));
        store.aux_cache = Some(aux_cache);
        store.edenapi = Some(SaplingRemoteApiFileStore::new(Arc::new(remote_repo)));
        store.cas_manager = Some(Arc::new(
            CasFetchManager::builder(cas_client.clone()).build(),
        ));

        let context = FetchContext::new(FetchMode::AllowRemote);
        let fetched = store
            .fetch(context.clone(), [key], FileAttributes::PURE_CONTENT)
            .single()?
            .expect("EdenAPI should satisfy the request without a content header");

        assert_eq!(fetched.file_content()?.into_bytes(), content_bytes);
        assert!(!context.fetch_from_cas_attempted());
        assert_eq!(cas_client.fetch_count.load(Ordering::Relaxed), 0);

        Ok(())
    }
}
