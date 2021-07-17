/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// TODO(meyer): Remove this
#![allow(dead_code)]
use std::collections::{hash_map, HashMap, HashSet};
use std::convert::{TryFrom, TryInto};
use std::ops::{AddAssign, BitAnd, BitOr, Not, Sub};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, ensure, Error, Result};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use tracing::{field, instrument};

use edenapi_types::{
    ContentId, FileAttributes as EdenApiFileAttributes, FileAuxData as EdenApiFileAuxData,
    FileEntry, FileSpec, Sha1,
};

use minibytes::Bytes;
use types::{HgId, Key, RepoPathBuf, Sha256};

use crate::{
    datastore::{strip_metadata, HgIdDataStore, HgIdMutableDeltaStore, RemoteDataStore},
    fetch_logger::FetchLogger,
    indexedlogauxstore::{AuxStore, Entry as AuxDataEntry},
    indexedlogdatastore::{Entry, IndexedLogHgIdDataStore},
    indexedlogutil::StoreType,
    lfs::{
        lfs_from_hg_file_blob, rebuild_metadata, LfsPointersEntry, LfsRemote, LfsRemoteInner,
        LfsStore, LfsStoreEntry,
    },
    memcache::McData,
    remotestore::HgIdRemoteStore,
    util, ContentDataStore, ContentHash, ContentMetadata, ContentStore, Delta, EdenApiFileStore,
    ExtStoredPolicy, LegacyStore, LocalStore, MemcacheStore, Metadata, MultiplexDeltaStore,
    RepackLocation, StoreKey, StoreResult,
};

#[derive(Clone, Debug, Default)]
struct FetchMetrics {
    /// Number of requests / batches
    requests: usize,

    /// Numbers of entities requested
    keys: usize,

    /// Number of successfully fetched entities
    hits: usize,

    /// Number of entities which were not found
    misses: usize,

    /// Number of entities which returned a fetch error (including batch errors)
    errors: usize,
}

impl AddAssign for FetchMetrics {
    fn add_assign(&mut self, rhs: Self) {
        self.requests += rhs.requests;
        self.keys += rhs.keys;
        self.hits += rhs.hits;
        self.misses += rhs.misses;
        self.errors += rhs.errors;
    }
}

impl FetchMetrics {
    fn fetch(&mut self, keys: usize) {
        self.requests += 1;
        self.keys += keys;
    }

    fn hit(&mut self, keys: usize) {
        self.hits += keys;
    }

    fn miss(&mut self, keys: usize) {
        self.misses += keys;
    }

    fn err(&mut self, keys: usize) {
        self.errors += keys;
    }

    fn metrics(&self) -> impl Iterator<Item = (&'static str, usize)> {
        std::array::IntoIter::new([
            ("requests", self.requests),
            ("keys", self.keys),
            ("hits", self.hits),
            ("misses", self.misses),
            ("errors", self.errors),
        ])
        .filter(|&(_, v)| v != 0)
    }
}

fn namespaced(
    namespace: &'static str,
    metrics: impl Iterator<Item = (impl AsRef<str>, usize)>,
) -> impl Iterator<Item = (String, usize)> {
    metrics.map(move |(k, v)| (namespace.to_string() + "." + k.as_ref(), v))
}

#[derive(Clone, Debug, Default)]
pub struct LocalAndCacheFetchMetrics {
    local: FetchMetrics,
    cache: FetchMetrics,
}

impl LocalAndCacheFetchMetrics {
    fn store(&mut self, typ: StoreType) -> &mut FetchMetrics {
        match typ {
            StoreType::Local => &mut self.local,
            StoreType::Shared => &mut self.cache,
        }
    }

    fn metrics(&self) -> impl Iterator<Item = (String, usize)> {
        namespaced("local", self.local.metrics()).chain(namespaced("cache", self.cache.metrics()))
    }
}

impl AddAssign for LocalAndCacheFetchMetrics {
    fn add_assign(&mut self, rhs: Self) {
        self.local += rhs.local;
        self.cache += rhs.cache;
    }
}

#[derive(Clone, Debug, Default)]
pub struct FileStoreFetchMetrics {
    indexedlog: LocalAndCacheFetchMetrics,
    lfs: LocalAndCacheFetchMetrics,
    aux: LocalAndCacheFetchMetrics,
    contentstore: FetchMetrics,
}

impl AddAssign for FileStoreFetchMetrics {
    fn add_assign(&mut self, rhs: Self) {
        self.indexedlog += rhs.indexedlog;
        self.lfs += rhs.lfs;
        self.aux += rhs.aux;
        self.contentstore += rhs.contentstore;
    }
}

impl FileStoreFetchMetrics {
    fn metrics(&self) -> impl Iterator<Item = (String, usize)> {
        namespaced("indexedlog", self.indexedlog.metrics())
            .chain(namespaced("lfs", self.lfs.metrics()))
            .chain(namespaced("aux", self.aux.metrics()))
            .chain(namespaced("contentstore", self.contentstore.metrics()))
    }
}

#[derive(Debug, Default, Clone)]
pub struct FileStoreMetrics {
    fetch: FileStoreFetchMetrics,
}

impl FileStoreMetrics {
    pub fn new() -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(FileStoreMetrics::default()))
    }

    pub fn metrics(&self) -> impl Iterator<Item = (String, usize)> {
        namespaced("scmstore.file.fetch", self.fetch.metrics())
    }
}

const MEMCACHE_DELAY: Duration = Duration::from_secs(10);

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
    pub(crate) fallbacks: Arc<ContentStoreFallbacks>,

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

#[derive(Debug, Default)]
struct ContentStoreFallbacksInner {
    fetch: u64,
    fetch_miss: u64,
    fetch_hit_ptr: u64,
    fetch_hit_content: u64,
    write_ptr: u64,
}

#[derive(Debug)]
pub struct ContentStoreFallbacks {
    inner: Mutex<ContentStoreFallbacksInner>,
}

impl ContentStoreFallbacks {
    pub(crate) fn new() -> Self {
        ContentStoreFallbacks {
            inner: Mutex::new(ContentStoreFallbacksInner::default()),
        }
    }

    #[instrument(level = "warn", skip(self))]
    fn fetch(&self, _key: &Key) {
        self.inner.lock().fetch += 1;
    }

    #[instrument(level = "warn", skip(self))]
    fn fetch_miss(&self, _key: &Key) {
        self.inner.lock().fetch_miss += 1;
    }

    #[instrument(level = "warn", skip(self))]
    fn fetch_hit_ptr(&self, _key: &Key) {
        self.inner.lock().fetch_hit_ptr += 1;
    }

    #[instrument(level = "warn", skip(self))]
    fn fetch_hit_content(&self, _key: &Key) {
        self.inner.lock().fetch_hit_content += 1;
    }

    #[instrument(level = "warn", skip(self))]
    fn write_ptr(&self, _key: &Key) {
        self.inner.lock().write_ptr += 1;
    }

    pub fn fetch_count(&self) -> u64 {
        self.inner.lock().fetch
    }

    pub fn fetch_miss_count(&self) -> u64 {
        self.inner.lock().fetch_miss
    }

    pub fn fetch_hit_ptr_count(&self) -> u64 {
        self.inner.lock().fetch_hit_ptr
    }

    pub fn fetch_hit_content_count(&self) -> u64 {
        self.inner.lock().fetch_hit_content
    }

    pub fn write_ptr_count(&self) -> u64 {
        self.inner.lock().write_ptr
    }
}

#[derive(Debug)]
pub struct FileStoreFetch {
    pub complete: HashMap<Key, StoreFile>,
    pub incomplete: HashMap<Key, Vec<Error>>,
    other_errors: Vec<Error>,
    metrics: FileStoreFetchMetrics,
}

impl FileStoreFetch {
    /// Return the list of keys which could not be fetched, or any errors encountered
    pub fn missing(mut self) -> Result<Vec<Key>> {
        if let Some(err) = self.other_errors.pop() {
            return Err(err).into();
        }

        let mut not_found = Vec::new();
        for (key, mut errors) in self.incomplete.drain() {
            if let Some(err) = errors.pop() {
                return Err(err).into();
            }
            not_found.push(key);
        }

        Ok(not_found)
    }

    /// Return the single requested file if found, or any errors encountered
    pub fn single(mut self) -> Result<Option<StoreFile>> {
        if let Some(err) = self.other_errors.pop() {
            return Err(err).into();
        }

        for (_key, mut errors) in self.incomplete.drain() {
            if let Some(err) = errors.pop() {
                return Err(err).into();
            } else {
                return Ok(None);
            }
        }

        Ok(Some(
            self.complete
                .drain()
                .next()
                .ok_or_else(|| anyhow!("no results found in either incomplete or complete"))?
                .1,
        ))
    }

    /// Returns a stream of all successful fetches and errors, for compatibility with old scmstore
    pub fn results(self) -> impl Iterator<Item = Result<(Key, StoreFile)>> {
        self.complete
            .into_iter()
            .map(Ok)
            .chain(
                self.incomplete
                    .into_iter()
                    .map(|(key, errors)| {
                        if errors.len() > 0 {
                            errors
                        } else {
                            vec![anyhow!("key not found: {}", key)]
                        }
                    })
                    .flatten()
                    .map(Err),
            )
            .chain(self.other_errors.into_iter().map(Err))
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
        self.metrics.write().fetch += fetched.metrics.clone();
        fetched
    }

    fn use_memcache(&self) -> bool {
        // Only use memcache if the process has been around a while. It takes 2s to setup, which
        // hurts responiveness for short commands.
        self.creation_time.elapsed() > MEMCACHE_DELAY
    }

    #[instrument(skip(self, entries))]
    pub fn write_batch(&self, entries: impl Iterator<Item = (Key, Bytes, Metadata)>) -> Result<()> {
        let mut indexedlog_local = self.indexedlog_local.as_ref().map(|l| l.write_lock());
        for (key, bytes, meta) in entries {
            if meta.is_lfs() {
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
                self.fallbacks.write_ptr(&key);
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
                continue;
            }
            let hg_blob_len = bytes.len() as u64;
            // Default to non-LFS if no LFS threshold is set
            if self
                .lfs_threshold_bytes
                .map_or(false, |threshold| hg_blob_len > threshold)
            {
                let lfs_local = self.lfs_local.as_ref().ok_or_else(|| {
                    anyhow!("trying to write LFS file but no local LfsStore is available")
                })?;
                let (lfs_pointer, lfs_blob) = lfs_from_hg_file_blob(key.hgid, &bytes)?;
                let sha256 = lfs_pointer.sha256();

                // TODO(meyer): Do similar LockGuard impl for LfsStore so we can lock across the batch for both
                lfs_local.add_blob(&sha256, lfs_blob)?;
                lfs_local.add_pointer(lfs_pointer)?;
            } else {
                let indexedlog_local = indexedlog_local.as_mut().ok_or_else(|| {
                    anyhow!(
                        "trying to write non-LFS file but no local non-LFS IndexedLog is available"
                    )
                })?;
                indexedlog_local.put_entry(Entry::new(key, bytes, meta))?;
            }
        }
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
            fallbacks: self.fallbacks.clone(),
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

    pub fn fallbacks(&self) -> Arc<ContentStoreFallbacks> {
        self.fallbacks.clone()
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
            fallbacks: Arc::new(ContentStoreFallbacks::new()),
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
            fallbacks: self.fallbacks.clone(),
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
        if let Some(contentstore) = self.contentstore.as_ref() {
            contentstore.commit_pending(location)
        } else {
            self.flush()?;
            Ok(None)
        }
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FileAuxData {
    pub total_size: u64,
    pub content_id: ContentId,
    pub content_sha1: Sha1,
    pub content_sha256: Sha256,
}

impl From<AuxDataEntry> for FileAuxData {
    fn from(v: AuxDataEntry) -> Self {
        FileAuxData {
            total_size: v.total_size() as u64,
            content_id: v.content_id(),
            content_sha1: v.content_sha1(),
            content_sha256: Sha256::from_byte_array(v.content_sha256().0),
        }
    }
}

impl From<FileAuxData> for AuxDataEntry {
    fn from(v: FileAuxData) -> Self {
        AuxDataEntry {
            total_size: v.total_size,
            content_id: v.content_id,
            content_sha1: v.content_sha1,
            content_sha256: v.content_sha256.into_inner().into(),
        }
    }
}

impl From<EdenApiFileAuxData> for FileAuxData {
    fn from(v: EdenApiFileAuxData) -> Self {
        FileAuxData {
            total_size: v.total_size,
            content_id: v.content_id,
            content_sha1: v.sha1,
            content_sha256: Sha256::from_byte_array(v.sha256.0),
        }
    }
}

impl From<FileAuxData> for EdenApiFileAuxData {
    fn from(v: FileAuxData) -> Self {
        EdenApiFileAuxData {
            total_size: v.total_size,
            content_id: v.content_id,
            sha1: v.content_sha1,
            sha256: v.content_sha256.into_inner().into(),
        }
    }
}

#[derive(Debug)]
pub struct StoreFile {
    // TODO(meyer): We'll probably eventually need a better "canonical lazy file" abstraction, since EdenApi FileEntry won't always carry content
    content: Option<LazyFile>,
    aux_data: Option<FileAuxData>,
}

impl StoreFile {
    /// Returns which attributes are present in this StoreFile
    fn attrs(&self) -> FileAttributes {
        FileAttributes {
            content: self.content.is_some(),
            aux_data: self.aux_data.is_some(),
        }
    }

    /// Return a StoreFile with only the specified subset of attributes
    fn mask(self, attrs: FileAttributes) -> Self {
        StoreFile {
            content: if attrs.content { self.content } else { None },
            aux_data: if attrs.aux_data { self.aux_data } else { None },
        }
    }

    pub fn aux_data(&self) -> Option<FileAuxData> {
        self.aux_data.clone()
    }

    #[instrument(level = "debug", skip(self))]
    fn compute_aux_data(&mut self) -> Result<()> {
        self.aux_data = Some(
            self.content
                .as_mut()
                .ok_or_else(|| anyhow!("failed to compute aux data, no content available"))?
                .aux_data()?,
        );
        Ok(())
    }

    #[instrument(skip(self))]
    pub fn file_content(&mut self) -> Result<Bytes> {
        self.content
            .as_mut()
            .ok_or_else(|| anyhow!("no content available"))?
            .file_content()
    }
}

impl BitOr for StoreFile {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        StoreFile {
            content: self.content.or(rhs.content),
            aux_data: self.aux_data.or(rhs.aux_data),
        }
    }
}

impl Default for StoreFile {
    fn default() -> Self {
        StoreFile {
            content: None,
            aux_data: None,
        }
    }
}

impl From<FileAuxData> for StoreFile {
    fn from(v: FileAuxData) -> Self {
        StoreFile {
            content: None,
            aux_data: Some(v),
        }
    }
}

impl From<LazyFile> for StoreFile {
    fn from(v: LazyFile) -> Self {
        StoreFile {
            content: Some(v),
            aux_data: None,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct FileAttributes {
    pub content: bool,
    pub aux_data: bool,
}

impl From<FileAttributes> for EdenApiFileAttributes {
    fn from(v: FileAttributes) -> Self {
        EdenApiFileAttributes {
            content: v.content,
            aux_data: v.aux_data,
        }
    }
}

impl FileAttributes {
    /// Returns all the attributes which are present or can be computed from present attributes.
    fn with_computable(&self) -> FileAttributes {
        if self.content {
            *self | FileAttributes::AUX
        } else {
            *self
        }
    }

    /// Returns true if all the specified attributes are set, otherwise false.
    fn has(&self, attrs: FileAttributes) -> bool {
        (attrs - *self).none()
    }

    /// Returns true if no attributes are set, otherwise false.
    fn none(&self) -> bool {
        *self == FileAttributes::NONE
    }

    /// Returns true if at least one attribute is set, otherwise false.
    fn any(&self) -> bool {
        *self != FileAttributes::NONE
    }

    /// Returns true if all attributes are set, otherwise false.
    fn all(&self) -> bool {
        !*self == FileAttributes::NONE
    }

    pub const NONE: Self = FileAttributes {
        content: false,
        aux_data: false,
    };

    pub const CONTENT: Self = FileAttributes {
        content: true,
        aux_data: false,
    };

    pub const AUX: Self = FileAttributes {
        content: false,
        aux_data: true,
    };
}

impl Not for FileAttributes {
    type Output = Self;

    fn not(self) -> Self::Output {
        FileAttributes {
            content: !self.content,
            aux_data: !self.aux_data,
        }
    }
}

impl BitAnd for FileAttributes {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        FileAttributes {
            content: self.content & rhs.content,
            aux_data: self.aux_data & rhs.aux_data,
        }
    }
}

impl BitOr for FileAttributes {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        FileAttributes {
            content: self.content | rhs.content,
            aux_data: self.aux_data | rhs.aux_data,
        }
    }
}

/// The subtraction operator is implemented here to mean "set difference" aka relative complement.
impl Sub for FileAttributes {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        self & !rhs
    }
}

/// A minimal file enum that simply wraps the possible underlying file types,
/// with no processing (so Entry might have the wrong Key.path, etc.)
#[derive(Debug)]
pub(crate) enum LazyFile {
    /// A response from calling into the legacy storage API
    ContentStore(Bytes, Metadata),

    /// An entry from a local IndexedLog. The contained Key's path might not match the requested Key's path.
    IndexedLog(Entry),

    /// A local LfsStore entry.
    Lfs(Bytes, LfsPointersEntry),

    /// An EdenApi FileEntry.
    EdenApi(FileEntry),

    /// A memcache entry, convertable to Entry. In this case the Key's path should match the requested Key's path.
    Memcache(McData),
}

impl LazyFile {
    fn hgid(&self) -> Option<HgId> {
        use LazyFile::*;
        match self {
            ContentStore(_, _) => None,
            IndexedLog(ref entry) => Some(entry.key().hgid),
            Lfs(_, ref ptr) => Some(ptr.hgid()),
            EdenApi(ref entry) => Some(entry.key().hgid),
            Memcache(ref entry) => Some(entry.key.hgid),
        }
    }

    /// Compute's the aux data associated with this file from the content.
    #[instrument(level = "debug", skip(self))]
    pub(crate) fn aux_data(&mut self) -> Result<FileAuxData> {
        // TODO(meyer): Implement the rest of the aux data fields
        Ok(if let LazyFile::Lfs(content, ref ptr) = self {
            FileAuxData {
                total_size: content.len() as u64,
                content_id: ContentHash::content_id(&content),
                content_sha1: ContentHash::sha1(&content),
                content_sha256: ptr.sha256(),
            }
        } else {
            let content = self.file_content()?;
            FileAuxData {
                total_size: content.len() as u64,
                content_id: ContentHash::content_id(&content),
                content_sha1: ContentHash::sha1(&content),
                content_sha256: ContentHash::sha256(&content).unwrap_sha256(),
            }
        })
    }

    /// The file content, as would be found in the working copy (stripped of copy header)
    #[instrument(level = "debug", skip(self))]
    fn file_content(&mut self) -> Result<Bytes> {
        use LazyFile::*;
        Ok(match self {
            IndexedLog(ref mut entry) => strip_metadata(&entry.content()?)?.0,
            Lfs(ref blob, _) => blob.clone(),
            ContentStore(ref blob, _) => strip_metadata(blob)?.0,
            // TODO(meyer): Convert EdenApi to use minibytes
            EdenApi(ref entry) => strip_metadata(&entry.data()?.into())?.0,
            Memcache(ref entry) => strip_metadata(&entry.data)?.0,
        })
    }

    /// The file content, as would be encoded in the Mercurial blob (with copy header)
    #[instrument(level = "debug", skip(self))]
    fn hg_content(&mut self) -> Result<Bytes> {
        use LazyFile::*;
        Ok(match self {
            IndexedLog(ref mut entry) => entry.content()?,
            Lfs(ref blob, ref ptr) => rebuild_metadata(blob.clone(), ptr),
            ContentStore(ref blob, _) => blob.clone(),
            EdenApi(ref entry) => entry.data()?.into(),
            Memcache(ref entry) => entry.data.clone(),
        })
    }

    #[instrument(level = "debug", skip(self))]
    fn metadata(&self) -> Result<Metadata> {
        use LazyFile::*;
        Ok(match self {
            IndexedLog(ref entry) => entry.metadata().clone(),
            Lfs(_, ref ptr) => Metadata {
                size: Some(ptr.size()),
                flags: None,
            },
            ContentStore(_, ref meta) => meta.clone(),
            EdenApi(ref entry) => entry.metadata()?.clone(),
            Memcache(ref entry) => entry.metadata.clone(),
        })
    }

    /// Convert the LazyFile to an indexedlog Entry, if it should ever be written to IndexedLog cache
    #[instrument(level = "debug", skip(self))]
    fn indexedlog_cache_entry(&self, key: Key) -> Result<Option<Entry>> {
        use LazyFile::*;
        Ok(match self {
            IndexedLog(ref entry) => Some(entry.clone().with_key(key)),
            EdenApi(ref entry) => Some(Entry::new(
                key,
                entry.data()?.into(),
                entry.metadata()?.clone(),
            )),
            // TODO(meyer): We shouldn't ever need to replace the key with Memcache, can probably just clone this.
            Memcache(ref entry) => Some({
                let entry: Entry = entry.clone().into();
                entry.with_key(key)
            }),
            // LFS Files should be written to LfsCache instead
            Lfs(_, _) => None,
            // ContentStore handles caching internally
            ContentStore(_, _) => None,
        })
    }
}

impl TryFrom<McData> for LfsPointersEntry {
    type Error = Error;

    fn try_from(e: McData) -> Result<Self, Self::Error> {
        if e.metadata.is_lfs() {
            Ok(LfsPointersEntry::from_bytes(e.data, e.key.hgid)?)
        } else {
            bail!("failed to convert McData entry to LFS pointer, is_lfs is false")
        }
    }
}

impl TryFrom<Entry> for LfsPointersEntry {
    type Error = Error;

    fn try_from(mut e: Entry) -> Result<Self, Self::Error> {
        if e.metadata().is_lfs() {
            Ok(LfsPointersEntry::from_bytes(e.content()?, e.key().hgid)?)
        } else {
            bail!("failed to convert entry to LFS pointer, is_lfs is false")
        }
    }
}

impl TryFrom<FileEntry> for LfsPointersEntry {
    type Error = Error;

    fn try_from(e: FileEntry) -> Result<Self, Self::Error> {
        if e.metadata()?.is_lfs() {
            Ok(LfsPointersEntry::from_bytes(e.data()?, e.key().hgid)?)
        } else {
            bail!("failed to convert EdenApi FileEntry to LFS pointer, but is_lfs is false")
        }
    }
}

pub struct FetchErrors {
    /// Errors encountered for specific keys
    fetch_errors: HashMap<Key, Vec<Error>>,

    /// Errors encountered that don't apply to a single key
    other_errors: Vec<Error>,
}

impl FetchErrors {
    fn new() -> Self {
        FetchErrors {
            fetch_errors: HashMap::new(),
            other_errors: Vec::new(),
        }
    }

    #[instrument(level = "error", skip(self))]
    fn keyed_error(&mut self, key: Key, err: Error) {
        self.fetch_errors
            .entry(key)
            .or_insert_with(Vec::new)
            .push(err);
    }

    #[instrument(level = "error", skip(self))]
    fn other_error(&mut self, err: Error) {
        self.other_errors.push(err);
    }
}

pub struct FetchState {
    /// Requested keys for which at least some attributes haven't been found.
    pending: HashSet<Key>,

    /// Which attributes were requested
    request_attrs: FileAttributes,

    /// All attributes which have been found so far
    found: HashMap<Key, StoreFile>,

    /// LFS pointers we've discovered corresponding to a request Key.
    lfs_pointers: HashMap<Key, LfsPointersEntry>,

    /// A table tracking if discovered LFS pointers were found in the local-only or cache / shared store.
    pointer_origin: Arc<RwLock<HashMap<Sha256, StoreType>>>,

    /// A table tracking if each key is local-only or cache/shared so that computed aux data can be written to the appropriate store
    key_origin: HashMap<Key, StoreType>,

    /// Errors encountered during fetching.
    errors: FetchErrors,

    /// File content found in memcache, may be cached locally (currently only content may be found in memcache)
    found_in_memcache: HashSet<Key>,

    /// Attributes found in EdenApi, may be cached locally (currently only content may be found in EdenApi)
    found_in_edenapi: HashSet<Key>,

    found_remote_aux: HashSet<Key>,

    /// Attributes computed from other attributes, may be cached locally (currently only aux_data may be computed)
    computed_aux_data: HashMap<Key, StoreType>,

    /// Tracks remote fetches which match a specific regex
    fetch_logger: Option<Arc<FetchLogger>>,

    /// Track ContentStore Fallbacks
    fallbacks: Arc<ContentStoreFallbacks>,

    /// Track fetch metrics,
    metrics: FileStoreFetchMetrics,

    // Config
    extstored_policy: ExtStoredPolicy,
    compute_aux_data: bool,
}

impl FetchState {
    fn new(keys: impl Iterator<Item = Key>, attrs: FileAttributes, file_store: &FileStore) -> Self {
        FetchState {
            pending: keys.collect(),
            request_attrs: attrs,

            found: HashMap::new(),

            lfs_pointers: HashMap::new(),
            key_origin: HashMap::new(),
            pointer_origin: Arc::new(RwLock::new(HashMap::new())),

            errors: FetchErrors::new(),

            found_in_memcache: HashSet::new(),
            found_in_edenapi: HashSet::new(),
            found_remote_aux: HashSet::new(),
            computed_aux_data: HashMap::new(),

            fetch_logger: file_store.fetch_logger.clone(),
            fallbacks: file_store.fallbacks.clone(),
            extstored_policy: file_store.extstored_policy,
            compute_aux_data: true,
            metrics: FileStoreFetchMetrics::default(),
        }
    }

    /// Return all incomplete requested Keys for which additional attributes may be gathered by querying a store which provides the specified attributes.
    fn pending_all(&self, fetchable: FileAttributes) -> Vec<Key> {
        if fetchable.none() {
            return vec![];
        }
        self.pending
            .iter()
            .filter(|k| self.actionable(k, fetchable).any())
            .cloned()
            .collect()
    }

    /// Returns all incomplete requested Keys for which we haven't discovered an LFS pointer, and for which additional attributes may be gathered by querying a store which provides the specified attributes.
    fn pending_nonlfs(&self, fetchable: FileAttributes) -> Vec<Key> {
        if fetchable.none() {
            return vec![];
        }
        self.pending
            .iter()
            .filter(|k| !self.lfs_pointers.contains_key(k))
            .filter(|k| self.actionable(k, fetchable).any())
            .cloned()
            .collect()
    }

    /// Returns all incomplete requested Keys as Store, with content Sha256 from the LFS pointer if available, for which additional attributes may be gathered by querying a store which provides the specified attributes
    fn pending_storekey(&self, fetchable: FileAttributes) -> Vec<StoreKey> {
        if fetchable.none() {
            return vec![];
        }
        self.pending
            .iter()
            .filter(|k| self.actionable(k, fetchable).any())
            .map(|k| self.storekey(k))
            .collect()
    }

    /// A key is actionable with respect to a store if we can fetch something that is or allows us to compute a missing attribute.
    #[instrument(level = "trace", skip(self))]
    fn actionable(&self, key: &Key, fetchable: FileAttributes) -> FileAttributes {
        if fetchable.none() {
            return FileAttributes::NONE;
        }

        let available = self
            .found
            .get(key)
            .map_or(FileAttributes::NONE, |f| f.attrs());
        let (available, fetchable) = if self.compute_aux_data {
            (available.with_computable(), fetchable.with_computable())
        } else {
            (available, fetchable)
        };
        let missing = self.request_attrs - available;
        let actionable = missing & fetchable;
        actionable
    }

    /// Returns the Key as a StoreKey, as a StoreKey::Content with Sha256 from the LFS Pointer, if available, otherwise as a StoreKey::HgId.
    /// Every StoreKey returned from this function is guaranteed to have an associated Key, so unwrapping is fine.
    fn storekey(&self, key: &Key) -> StoreKey {
        self.lfs_pointers.get(key).map_or_else(
            || StoreKey::HgId(key.clone()),
            |ptr| StoreKey::Content(ContentHash::Sha256(ptr.sha256()), Some(key.clone())),
        )
    }

    #[instrument(level = "debug", skip(self))]
    fn mark_complete(&mut self, key: &Key) {
        self.pending.remove(key);
        if let Some(ptr) = self.lfs_pointers.remove(key) {
            self.pointer_origin.write().remove(&ptr.sha256());
        }
    }

    #[instrument(level = "debug", skip(self, ptr))]
    fn found_pointer(&mut self, key: Key, ptr: LfsPointersEntry, typ: StoreType) {
        let sha256 = ptr.sha256();
        // Overwrite StoreType::Local with StoreType::Shared, but not vice versa
        match typ {
            StoreType::Shared => {
                self.pointer_origin.write().insert(sha256, typ);
            }
            StoreType::Local => {
                self.pointer_origin.write().entry(sha256).or_insert(typ);
            }
        }
        self.lfs_pointers.insert(key, ptr);
    }

    #[instrument(level = "debug", skip(self, sf))]
    fn found_attributes(&mut self, key: Key, sf: StoreFile, typ: Option<StoreType>) {
        self.key_origin
            .insert(key.clone(), typ.unwrap_or(StoreType::Shared));
        use hash_map::Entry::*;
        match self.found.entry(key.clone()) {
            Occupied(mut entry) => {
                tracing::debug!("merging into previously fetched attributes");
                // Combine the existing and newly-found attributes, overwriting existing attributes with the new ones
                // if applicable (so that we can re-use this function to replace in-memory files with mmap-ed files)
                let available = entry.get_mut();
                *available = sf | std::mem::take(available);

                if available.attrs().has(self.request_attrs) {
                    self.mark_complete(&key);
                }
            }
            Vacant(entry) => {
                if entry.insert(sf).attrs().has(self.request_attrs) {
                    self.mark_complete(&key);
                }
            }
        };
    }

    #[instrument(level = "debug", skip(self, entry))]
    fn found_indexedlog(&mut self, key: Key, entry: Entry, typ: StoreType) {
        if entry.metadata().is_lfs() {
            if self.extstored_policy == ExtStoredPolicy::Use {
                match entry.try_into() {
                    Ok(ptr) => self.found_pointer(key, ptr, typ),
                    Err(err) => self.errors.keyed_error(key, err),
                }
            }
        } else {
            self.found_attributes(key, LazyFile::IndexedLog(entry).into(), Some(typ))
        }
    }

    #[instrument(skip(self, store))]
    fn fetch_indexedlog(&mut self, store: &IndexedLogHgIdDataStore, typ: StoreType) {
        let pending = self.pending_nonlfs(FileAttributes::CONTENT);
        if pending.is_empty() {
            return;
        }
        self.metrics.indexedlog.store(typ).fetch(pending.len());
        let store = store.read_lock();
        for key in pending.into_iter() {
            let res = store.get_raw_entry(&key);
            match res {
                Ok(Some(entry)) => {
                    self.metrics.indexedlog.store(typ).hit(1);
                    self.found_indexedlog(key, entry, typ)
                }
                Ok(None) => {
                    self.metrics.indexedlog.store(typ).miss(1);
                }
                Err(err) => {
                    self.metrics.indexedlog.store(typ).err(1);
                    self.errors.keyed_error(key, err)
                }
            }
        }
    }

    #[instrument(level = "debug", skip(self, entry))]
    fn found_aux_indexedlog(&mut self, key: Key, entry: AuxDataEntry, typ: StoreType) {
        let aux_data: FileAuxData = entry.into();
        self.found_attributes(key, aux_data.into(), Some(typ));
    }

    #[instrument(skip(self, store))]
    fn fetch_aux_indexedlog(&mut self, store: &AuxStore, typ: StoreType) {
        let pending = self.pending_all(FileAttributes::AUX);
        if pending.is_empty() {
            return;
        }
        self.metrics.aux.store(typ).fetch(pending.len());

        let store = store.read();
        for key in pending.into_iter() {
            let res = store.get(key.hgid);
            match res {
                Ok(Some(aux)) => {
                    self.metrics.aux.store(typ).hit(1);
                    self.found_aux_indexedlog(key, aux, typ)
                }
                Ok(None) => {
                    self.metrics.aux.store(typ).miss(1);
                }
                Err(err) => {
                    self.metrics.aux.store(typ).err(1);
                    self.errors.keyed_error(key, err)
                }
            }
        }
    }

    #[instrument(level = "debug", skip(self, entry))]
    fn found_lfs(&mut self, key: Key, entry: LfsStoreEntry, typ: StoreType) {
        match entry {
            LfsStoreEntry::PointerAndBlob(ptr, blob) => {
                self.found_attributes(key, LazyFile::Lfs(blob, ptr).into(), Some(typ))
            }
            LfsStoreEntry::PointerOnly(ptr) => self.found_pointer(key, ptr, typ),
        }
    }

    #[instrument(skip(self, store))]
    fn fetch_lfs(&mut self, store: &LfsStore, typ: StoreType) {
        let pending = self.pending_storekey(FileAttributes::CONTENT);
        if pending.is_empty() {
            return;
        }
        self.metrics.lfs.store(typ).fetch(pending.len());
        for store_key in pending.into_iter() {
            let key = store_key.clone().maybe_into_key().expect(
                "no Key present in StoreKey, even though this should be guaranteed by pending_all",
            );
            match store.fetch_available(&store_key) {
                Ok(Some(entry)) => {
                    // TODO(meyer): Make found behavior w/r/t LFS pointers and content consistent
                    self.metrics.lfs.store(typ).hit(1);
                    self.found_lfs(key, entry, typ)
                }
                Ok(None) => {
                    self.metrics.lfs.store(typ).miss(1);
                }
                Err(err) => {
                    self.metrics.lfs.store(typ).err(1);
                    self.errors.keyed_error(key, err)
                }
            }
        }
    }

    #[instrument(level = "debug", skip(self, entry))]
    fn found_memcache(&mut self, entry: McData) {
        let key = entry.key.clone();
        if entry.metadata.is_lfs() {
            match entry.try_into() {
                Ok(ptr) => self.found_pointer(key, ptr, StoreType::Shared),
                Err(err) => self.errors.keyed_error(key, err),
            }
        } else {
            self.found_in_memcache.insert(key.clone());
            self.found_attributes(key, LazyFile::Memcache(entry).into(), None);
        }
    }

    fn fetch_memcache_inner(&mut self, store: &MemcacheStore) -> Result<()> {
        let pending = self.pending_nonlfs(FileAttributes::CONTENT);
        if pending.is_empty() {
            return Ok(());
        }
        self.fetch_logger
            .as_ref()
            .map(|fl| fl.report_keys(pending.iter()));

        for res in store.get_data_iter(&pending)?.into_iter() {
            match res {
                Ok(mcdata) => self.found_memcache(mcdata),
                Err(err) => self.errors.other_error(err),
            }
        }
        Ok(())
    }

    #[instrument(skip(self, store))]
    fn fetch_memcache(&mut self, store: &MemcacheStore) {
        if let Err(err) = self.fetch_memcache_inner(store) {
            self.errors.other_error(err);
        }
    }

    #[instrument(level = "debug", skip(self, entry))]
    fn found_edenapi(&mut self, entry: FileEntry) {
        let key = entry.key.clone();
        if let Some(aux_data) = entry.aux_data() {
            self.found_remote_aux.insert(key.clone());
            let aux_data: FileAuxData = aux_data.clone().into();
            self.found_attributes(key.clone(), aux_data.into(), None);
        }
        if let Some(content) = entry.content() {
            if content.metadata().is_lfs() {
                match entry.try_into() {
                    Ok(ptr) => self.found_pointer(key, ptr, StoreType::Shared),
                    Err(err) => self.errors.keyed_error(key, err),
                }
            } else {
                self.found_in_edenapi.insert(key.clone());
                // TODO(meyer): Refactor LazyFile to hold a FileContent instead of FileEntry
                self.found_attributes(key, LazyFile::EdenApi(entry).into(), None);
            }
        }
    }

    fn fetch_edenapi_inner(&mut self, store: &EdenApiFileStore) -> Result<()> {
        let fetchable = FileAttributes::CONTENT | FileAttributes::AUX;
        let span = tracing::info_span!(
            "fetch_edenapi",
            downloaded = field::Empty,
            uploaded = field::Empty,
            requests = field::Empty,
            time = field::Empty,
            latency = field::Empty,
            download_speed = field::Empty,
            scmstore = true,
        );
        let _enter = span.enter();

        let pending = self.pending_nonlfs(fetchable);
        if pending.is_empty() {
            return Ok(());
        }
        self.fetch_logger
            .as_ref()
            .map(|fl| fl.report_keys(pending.iter()));

        // TODO(meyer): Iterators or otherwise clean this up
        let pending_attrs: Vec<_> = pending
            .into_iter()
            .map(|k| {
                let actionable = self.actionable(&k, fetchable);
                FileSpec {
                    key: k,
                    attrs: actionable.into(),
                }
            })
            .collect();

        let response = store.files_attrs_blocking(pending_attrs, None)?;
        for entry in response.entries.into_iter() {
            self.found_edenapi(entry);
        }
        util::record_edenapi_stats(&span, &response.stats);

        Ok(())
    }

    fn fetch_edenapi(&mut self, store: &EdenApiFileStore) {
        if let Err(err) = self.fetch_edenapi_inner(store) {
            self.errors.other_error(err);
        }
    }

    fn fetch_lfs_remote_inner(
        &mut self,
        store: &LfsRemoteInner,
        local: Option<Arc<LfsStore>>,
        cache: Option<Arc<LfsStore>>,
    ) -> Result<()> {
        let pending: HashSet<_> = self
            .lfs_pointers
            .iter()
            .map(|(_k, v)| (v.sha256(), v.size() as usize))
            .collect();
        if pending.is_empty() {
            return Ok(());
        }
        self.fetch_logger
            .as_ref()
            .map(|fl| fl.report_keys(self.lfs_pointers.keys()));

        // Fetch & write to local LFS stores
        store.batch_fetch(&pending, {
            let lfs_local = local.clone();
            let lfs_cache = cache.clone();
            let pointer_origin = self.pointer_origin.clone();
            move |sha256, data| -> Result<()> {
                match pointer_origin.read().get(&sha256).ok_or_else(|| {
                    anyhow!(
                        "no source found for Sha256; received unexpected Sha256 from LFS server"
                    )
                })? {
                    StoreType::Local => lfs_local
                        .as_ref()
                        .expect("no lfs_local present when handling local LFS pointer")
                        .add_blob(&sha256, data),
                    StoreType::Shared => lfs_cache
                        .as_ref()
                        .expect("no lfs_cache present when handling cache LFS pointer")
                        .add_blob(&sha256, data),
                }
            }
        })?;

        // After prefetching into the local LFS stores, retry fetching from them. The returned Bytes will then be mmaps rather
        // than large files stored in memory.
        // TODO(meyer): We probably want to intermingle this with the remote fetch handler to avoid files being evicted between there
        // and here, rather than just retrying the local fetches.
        if let Some(ref lfs_cache) = cache {
            self.fetch_lfs(lfs_cache, StoreType::Shared)
        }

        if let Some(ref lfs_local) = local {
            self.fetch_lfs(lfs_local, StoreType::Local)
        }

        Ok(())
    }

    #[instrument(skip(self, store, local, cache), fields(local = local.is_some(), cache = cache.is_some()))]
    fn fetch_lfs_remote(
        &mut self,
        store: &LfsRemoteInner,
        local: Option<Arc<LfsStore>>,
        cache: Option<Arc<LfsStore>>,
    ) {
        if let Err(err) = self.fetch_lfs_remote_inner(store, local, cache) {
            self.errors.other_error(err);
        }
    }

    #[instrument(level = "debug", skip(self, bytes))]
    fn found_contentstore(&mut self, key: Key, bytes: Vec<u8>, meta: Metadata) {
        if meta.is_lfs() {
            self.fallbacks.fetch_hit_ptr(&key);
            // Do nothing. We're trying to avoid exposing LFS pointers to the consumer of this API.
            // We very well may need to expose LFS Pointers to the caller in the end (to match ContentStore's
            // ExtStoredPolicy behavior), but hopefully not, and if so we'll need to make it type safe.
            tracing::warn!("contentstore fallback returned serialized lfs pointer");
        } else {
            tracing::warn!(
                "contentstore fetched a file scmstore couldn't, \
                this indicates a bug or unsupported configuration: \
                fetched key '{}', found {} bytes of content with metadata {:?}.",
                key,
                bytes.len(),
                meta,
            );
            self.fallbacks.fetch_hit_content(&key);
            self.found_attributes(key, LazyFile::ContentStore(bytes.into(), meta).into(), None)
        }
    }

    fn fetch_contentstore_inner(&mut self, store: &ContentStore) -> Result<()> {
        let pending = self.pending_storekey(FileAttributes::CONTENT);
        if pending.is_empty() {
            return Ok(());
        }
        self.metrics.contentstore.fetch(pending.len());
        store.prefetch(&pending)?;
        for store_key in pending.into_iter() {
            let key = store_key.clone().maybe_into_key().expect(
                "no Key present in StoreKey, even though this should be guaranteed by pending_storekey",
            );
            self.fallbacks.fetch(&key);
            // Using the ContentStore API, fetch the hg file blob, then, if it's found, also fetch the file metadata.
            // Returns the requested file as Result<(Option<Vec<u8>>, Option<Metadata>)>
            // Produces a Result::Err if either the blob or metadata get returned an error
            let res = store
                .get(store_key.clone())
                .map(|store_result| store_result.into())
                .and_then({
                    let store_key = store_key.clone();
                    |maybe_blob| {
                        Ok((
                            maybe_blob,
                            store
                                .get_meta(store_key)
                                .map(|store_result| store_result.into())?,
                        ))
                    }
                });

            match res {
                Ok((Some(blob), Some(meta))) => {
                    self.metrics.contentstore.hit(1);
                    self.found_contentstore(key, blob, meta)
                }
                Err(err) => {
                    self.metrics.contentstore.err(1);
                    self.fallbacks.fetch_miss(&key);
                    self.errors.keyed_error(key, err)
                }
                _ => {
                    self.metrics.contentstore.miss(1);
                    self.fallbacks.fetch_miss(&key);
                }
            }
        }

        Ok(())
    }

    #[instrument(skip(self, store))]
    fn fetch_contentstore(&mut self, store: &ContentStore) {
        if let Err(err) = self.fetch_contentstore_inner(store) {
            self.errors.other_error(err);
        }
    }

    #[instrument(skip(self))]
    fn derive_computable(&mut self) {
        if !self.compute_aux_data {
            return;
        }

        for (key, value) in self.found.iter_mut() {
            let span = tracing::debug_span!("checking derivations", %key);
            let _guard = span.enter();

            let missing = self.request_attrs - value.attrs();
            let actionable = value.attrs().with_computable() & missing;

            if actionable.aux_data {
                tracing::debug!("computing aux data");
                if let Err(err) = value.compute_aux_data() {
                    self.errors.keyed_error(key.clone(), err);
                } else {
                    tracing::debug!("computed aux data");
                    self.computed_aux_data
                        .insert(key.clone(), self.key_origin[key]);
                }
            }

            // mark complete if applicable
            if value.attrs().has(self.request_attrs) {
                tracing::debug!("marking complete");
                // TODO(meyer): Extract out a "FetchPending" object like FetchErrors, or otherwise make it possible
                // to share a "mark complete" implementation while holding a mutable reference to self.found.
                self.pending.remove(key);
                if let Some(ptr) = self.lfs_pointers.remove(key) {
                    self.pointer_origin.write().remove(&ptr.sha256());
                }
            }
        }
    }

    // TODO(meyer): Improve how local caching works. At the very least do this in the background.
    // TODO(meyer): Log errors here instead of just ignoring.
    #[instrument(
        skip(self, indexedlog_cache, memcache, aux_cache, aux_local),
        fields(
            indexedlog_cache = indexedlog_cache.is_some(),
            memcache = memcache.is_some(),
            aux_cache = aux_cache.is_some(),
            aux_local = aux_local.is_some()))]
    fn write_to_cache(
        &mut self,
        indexedlog_cache: Option<&IndexedLogHgIdDataStore>,
        memcache: Option<&MemcacheStore>,
        aux_cache: Option<&AuxStore>,
        aux_local: Option<&AuxStore>,
    ) {
        let mut indexedlog_cache = indexedlog_cache.map(|s| s.write_lock());
        let mut aux_cache = aux_cache.map(|s| s.write());
        let mut aux_local = aux_local.map(|s| s.write());

        {
            let span = tracing::trace_span!("edenapi");
            let _guard = span.enter();
            for key in self.found_in_edenapi.drain() {
                if let Some(lazy_file) = self.found[&key].content.as_ref() {
                    if let Ok(Some(cache_entry)) = lazy_file.indexedlog_cache_entry(key) {
                        if let Some(memcache) = memcache {
                            if let Ok(mcdata) = cache_entry.clone().try_into() {
                                memcache.add_mcdata(mcdata)
                            }
                        }
                        if let Some(ref mut indexedlog_cache) = indexedlog_cache {
                            let _ = indexedlog_cache.put_entry(cache_entry);
                        }
                    }
                }
            }
        }

        {
            let span = tracing::trace_span!("memcache");
            let _guard = span.enter();
            for key in self.found_in_memcache.drain() {
                if let Some(lazy_file) = self.found[&key].content.as_ref() {
                    if let Ok(Some(cache_entry)) = lazy_file.indexedlog_cache_entry(key) {
                        if let Some(ref mut indexedlog_cache) = indexedlog_cache {
                            let _ = indexedlog_cache.put_entry(cache_entry);
                        }
                    }
                }
            }
        }

        {
            let span = tracing::trace_span!("remote_aux");
            let _guard = span.enter();
            for key in self.found_remote_aux.drain() {
                let entry: AuxDataEntry = self.found[&key].aux_data.unwrap().into();

                if let Some(ref mut aux_cache) = aux_cache {
                    let _ = aux_cache.put(key.hgid, &entry);
                }
            }
        }

        {
            let span = tracing::trace_span!("computed");
            let _guard = span.enter();
            for (key, origin) in self.computed_aux_data.drain() {
                let entry: AuxDataEntry = self.found[&key].aux_data.unwrap().into();
                match origin {
                    StoreType::Shared => {
                        if let Some(ref mut aux_cache) = aux_cache {
                            let _ = aux_cache.put(key.hgid, &entry);
                        }
                    }
                    StoreType::Local => {
                        if let Some(ref mut aux_local) = aux_local {
                            let _ = aux_local.put(key.hgid, &entry);
                        }
                    }
                }
            }
        }
    }

    #[instrument(skip(self))]
    fn finish(mut self) -> FileStoreFetch {
        // Combine and collect errors
        let mut incomplete = self.errors.fetch_errors;
        for key in self.pending.into_iter() {
            self.found.remove(&key);
            incomplete.entry(key).or_insert_with(Vec::new);
        }

        for (key, value) in self.found.iter_mut() {
            // Remove attributes that weren't requested (content only used to compute attributes)
            *value = std::mem::take(value).mask(self.request_attrs);

            // Don't return errors for keys we eventually found.
            incomplete.remove(key);
        }

        FileStoreFetch {
            complete: self.found,
            incomplete,
            other_errors: self.errors.other_errors,
            metrics: self.metrics,
        }
    }
}

impl HgIdDataStore for FileStore {
    // Fetch the raw content of a single TreeManifest blob
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
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
        // AFAIK refresh only matters for DataPack / PackStore
        Ok(())
    }
}

impl RemoteDataStore for FileStore {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
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
        self.flush()?;
        Ok(None)
    }
}

// TODO(meyer): Content addressing not supported at all for trees. I could look for HgIds present here and fetch with
// that if available, but I feel like there's probably something wrong if this is called for trees.
impl ContentDataStore for FileStore {
    fn blob(&self, key: StoreKey) -> Result<StoreResult<Bytes>> {
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
