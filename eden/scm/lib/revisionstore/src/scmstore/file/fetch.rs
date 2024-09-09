/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use anyhow::anyhow;
use anyhow::Result;
use async_runtime::block_on;
use async_runtime::spawn_blocking;
use async_runtime::stream_to_iter;
use cas_client::CasClient;
use clientinfo::get_client_request_info_thread_local;
use clientinfo_async::with_client_request_info_scope;
use crossbeam::channel::Sender;
use edenapi_types::FileResponse;
use edenapi_types::FileSpec;
use futures::StreamExt;
use futures::TryFutureExt;
use minibytes::Bytes;
use progress_model::AggregatingProgressBar;
use tracing::debug;
use tracing::field;
use types::errors::NetworkError;
use types::fetch_mode::FetchMode;
use types::CasDigest;
use types::CasDigestType;
use types::Key;
use types::Sha256;

use crate::error::ClonableError;
use crate::indexedlogauxstore::AuxStore;
use crate::indexedlogdatastore::Entry;
use crate::indexedlogdatastore::IndexedLogHgIdDataStore;
use crate::lfs::LfsPointersEntry;
use crate::lfs::LfsRemote;
use crate::lfs::LfsStore;
use crate::lfs::LfsStoreEntry;
use crate::scmstore::attrs::StoreAttrs;
use crate::scmstore::fetch::CommonFetchState;
use crate::scmstore::fetch::FetchErrors;
use crate::scmstore::fetch::KeyFetchError;
use crate::scmstore::file::metrics::FileStoreFetchMetrics;
use crate::scmstore::file::LazyFile;
use crate::scmstore::metrics::StoreLocation;
use crate::scmstore::value::StoreValue;
use crate::scmstore::FileAttributes;
use crate::scmstore::FileAuxData;
use crate::scmstore::FileStore;
use crate::scmstore::StoreFile;
use crate::util;
use crate::ContentHash;
use crate::ExtStoredPolicy;
use crate::Metadata;
use crate::SaplingRemoteApiFileStore;
use crate::StoreKey;

pub struct FetchState {
    common: CommonFetchState<StoreFile>,

    /// Errors encountered during fetching.
    errors: FetchErrors,

    /// LFS pointers we've discovered corresponding to a request Key.
    lfs_pointers: HashMap<Key, (LfsPointersEntry, bool)>,

    lfs_progress: Arc<AggregatingProgressBar>,

    /// Track fetch metrics,
    metrics: FileStoreFetchMetrics,

    // Config
    extstored_policy: ExtStoredPolicy,
    compute_aux_data: bool,

    lfs_enabled: bool,

    fetch_mode: FetchMode,
}

impl FetchState {
    pub(crate) fn new(
        keys: impl IntoIterator<Item = Key>,
        attrs: FileAttributes,
        file_store: &FileStore,
        found_tx: Sender<Result<(Key, StoreFile), KeyFetchError>>,
        lfs_enabled: bool,
        fetch_mode: FetchMode,
    ) -> Self {
        FetchState {
            common: CommonFetchState::new(keys, attrs, found_tx, fetch_mode),
            errors: FetchErrors::new(),
            metrics: FileStoreFetchMetrics::default(),

            lfs_pointers: HashMap::new(),

            extstored_policy: file_store.extstored_policy,
            compute_aux_data: file_store.compute_aux_data,
            lfs_progress: file_store.lfs_progress.clone(),
            lfs_enabled,
            fetch_mode,
        }
    }

    pub(crate) fn pending_len(&self) -> usize {
        self.common.pending_len()
    }

    pub(crate) fn all_keys(&self) -> Vec<Key> {
        self.common.pending.keys().cloned().collect()
    }

    pub(crate) fn metrics(&self) -> &FileStoreFetchMetrics {
        &self.metrics
    }

    /// Returns all incomplete requested Keys for which we haven't discovered an LFS pointer, and for which additional attributes may be gathered by querying a store which provides the specified attributes.
    fn pending_nonlfs(&self, fetchable: FileAttributes) -> Vec<Key> {
        if fetchable.none() {
            return vec![];
        }
        self.common
            .pending(fetchable, self.compute_aux_data)
            .map(|(key, _attrs)| key.clone())
            .filter(|k| !self.lfs_pointers.contains_key(k))
            .collect()
    }

    /// Returns all incomplete requested Keys as Store, with content Sha256 from the LFS pointer if available, for which additional attributes may be gathered by querying a store which provides the specified attributes
    fn pending_storekey(&self, fetchable: FileAttributes) -> Vec<StoreKey> {
        if fetchable.none() {
            return vec![];
        }
        self.common
            .pending(fetchable, self.compute_aux_data)
            .map(|(key, _attrs)| key.clone())
            .map(|k| self.storekey(k))
            .collect()
    }

    /// Returns the Key as a StoreKey, as a StoreKey::Content with Sha256 from the LFS Pointer, if available, otherwise as a StoreKey::HgId.
    /// Every StoreKey returned from this function is guaranteed to have an associated Key, so unwrapping is fine.
    fn storekey(&self, key: Key) -> StoreKey {
        if let Some((ptr, _)) = self.lfs_pointers.get(&key) {
            StoreKey::Content(ContentHash::Sha256(ptr.sha256()), Some(key))
        } else {
            StoreKey::HgId(key)
        }
    }

    fn found_pointer(&mut self, key: Key, ptr: LfsPointersEntry, write: bool) {
        self.lfs_pointers.insert(key, (ptr, write));
    }

    fn found_attributes(&mut self, key: Key, sf: StoreFile) {
        self.common.found(key.clone(), sf);
    }

    fn evict_to_cache(
        key: Key,
        file: LazyFile,
        indexedlog_cache: &IndexedLogHgIdDataStore,
    ) -> Result<LazyFile> {
        let cache_entry = file.indexedlog_cache_entry(key.clone())?.ok_or_else(|| {
            anyhow!(
                "expected LazyFile::SaplingRemoteApi, other LazyFile variants should not be written to cache"
            )
        })?;
        indexedlog_cache.put_entry(cache_entry)?;
        let mmap_entry = indexedlog_cache
            .get_entry(key)?
            .ok_or_else(|| anyhow!("failed to read entry back from indexedlog after writing"))?;
        Ok(LazyFile::IndexedLog(mmap_entry))
    }

    fn ugprade_lfs_pointers(&mut self, entries: Vec<(Key, Entry)>, lfs_store: Option<&LfsStore>) {
        for (key, entry) in entries {
            match entry.try_into() {
                Ok(ptr) => {
                    if let Some(lfs_store) = lfs_store {
                        // Promote this indexedlog LFS pointer to the
                        // pointer store if it isn't already present. This
                        // should only happen when the Python LFS extension
                        // is in play.
                        if let Ok(None) = lfs_store
                            .fetch_available(&key.clone().into(), self.fetch_mode.ignore_result())
                        {
                            if let Err(err) = lfs_store.add_pointer(ptr) {
                                self.errors.keyed_error(key, err);
                            }
                        }
                    } else {
                        // If we don't have somewhere to upgrade pointer,
                        // track as a "found" pointer so it will be fetched
                        // from the remote store subsequently.
                        self.found_pointer(key, ptr, true)
                    }
                }
                Err(err) => self.errors.keyed_error(key, err),
            }
        }
    }

    pub(crate) fn fetch_indexedlog(
        &mut self,
        store: &IndexedLogHgIdDataStore,
        lfs_store: Option<&LfsStore>,
        loc: StoreLocation,
    ) {
        let pending = self.pending_nonlfs(FileAttributes::CONTENT);
        if pending.is_empty() {
            return;
        }

        let fetch_start = std::time::Instant::now();

        debug!(
            "Checking store Indexedlog ({cache}) for {key}{more}",
            cache = match loc {
                StoreLocation::Cache => "cache",
                StoreLocation::Local => "local",
            },
            key = pending[0],
            more = if pending.len() > 1 {
                format!(" and {} more", pending.len() - 1)
            } else {
                "".into()
            },
        );

        let mut found = 0;
        let mut count = 0;
        let mut errors = 0;
        let mut error: Option<String> = None;
        let mut lfs_pointers_to_upgrade = Vec::new();

        self.metrics.indexedlog.store(loc).fetch(pending.len());

        self.common
            .iter_pending(FileAttributes::CONTENT, self.compute_aux_data, |key| {
                count += 1;

                let res = if self.fetch_mode.ignore_result() {
                    store.contains(&key.hgid).map(|contains| {
                        if contains {
                            // Insert a stub entry if caller is ignoring the results.
                            Some(Entry::new(key.clone(), Bytes::new(), Metadata::default()))
                        } else {
                            None
                        }
                    })
                } else {
                    store.get_raw_entry(&key.hgid)
                };

                match res {
                    Ok(Some(entry)) => {
                        self.metrics.indexedlog.store(loc).hit(1);
                        found += 1;

                        if entry.metadata().is_lfs() && self.lfs_enabled {
                            // This is mainly for tests. We are handling the transition
                            // from the Python lfs extension (which stored pointers in the
                            // regular file store), the remotefilelog lfs implementation
                            // (which stores pointers in a separate store).
                            if self.extstored_policy == ExtStoredPolicy::Use {
                                lfs_pointers_to_upgrade.push((key.clone(), entry));
                            }
                        } else {
                            return Some(LazyFile::IndexedLog(entry).into());
                        }
                    }
                    Ok(None) => {
                        self.metrics.indexedlog.store(loc).miss(1);
                    }
                    Err(err) => {
                        self.metrics.indexedlog.store(loc).err(1);
                        errors += 1;
                        if error.is_none() {
                            error.replace(format!("{}: {}", key, err));
                        }
                        self.errors.keyed_error(key.clone(), err);
                    }
                }

                None
            });

        self.ugprade_lfs_pointers(lfs_pointers_to_upgrade, lfs_store);

        self.metrics
            .indexedlog
            .store(loc)
            .time_from_duration(fetch_start.elapsed())
            .ok();

        if found != 0 {
            debug!(
                "    Found {found} {result}",
                found = found,
                result = if found == 1 { "result" } else { "results" }
            );
        }
        if errors != 0 {
            debug!(
                "    Errors = {errors}, Error = {error:?}",
                errors = errors,
                error = error
            );
        }
    }

    pub(crate) fn fetch_aux_indexedlog(
        &mut self,
        store: &AuxStore,
        loc: StoreLocation,
        have_cas: bool,
    ) {
        let fetch_start = std::time::Instant::now();

        let mut found = 0;
        let mut errors = 0;
        let mut count = 0;
        let mut error: Option<String> = None;
        let ignore_results = self.fetch_mode.ignore_result() && !have_cas;

        let mut wants_aux = FileAttributes::AUX;
        if have_cas && loc == StoreLocation::Cache {
            // Also fetch AUX data if we are going to try fetching from CAS. This does two things:
            // 1. Fetches hash and size info needed to query CAS for file contents.
            // 2. Fetches hg content header, which is not available from CAS.
            wants_aux |= FileAttributes::PURE_CONTENT;
        }

        self.common
            .iter_pending(wants_aux, self.compute_aux_data, |key| {
                count += 1;

                let res = if ignore_results {
                    store.contains(key.hgid).map(|contains| {
                        if contains {
                            // Insert a stub entry if caller is ignoring the results.
                            Some(FileAuxData::default())
                        } else {
                            None
                        }
                    })
                } else {
                    store.get(key.hgid)
                };
                match res {
                    Ok(Some(aux)) => {
                        if have_cas {
                            tracing::trace!(target: "cas", ?key, ?aux, "found file aux data");
                        }
                        self.metrics.aux.store(loc).hit(1);
                        found += 1;
                        return Some(aux.into());
                    }
                    Ok(None) => {
                        if have_cas {
                            tracing::trace!(target: "cas", ?key, "no file aux data");
                        }
                        self.metrics.aux.store(loc).miss(1);
                    }
                    Err(err) => {
                        self.metrics.aux.store(loc).err(1);
                        errors += 1;
                        if error.is_none() {
                            error.replace(format!("{}: {}", key, err));
                        }
                        self.errors.keyed_error(key.clone(), err)
                    }
                }

                None
            });

        if count == 0 {
            return;
        }

        debug!(
            "Checking store AUX ({cache}) - Count = {count}",
            cache = match loc {
                StoreLocation::Cache => "cache",
                StoreLocation::Local => "local",
            },
        );

        self.metrics.aux.store(loc).fetch(count);

        self.metrics
            .aux
            .store(loc)
            .time_from_duration(fetch_start.elapsed())
            .ok();

        if found != 0 {
            debug!("    Found = {found}", found = found);
        }
        if errors != 0 {
            debug!(
                "    Errors = {errors}, Error = {error:?}",
                errors = errors,
                error = error
            );
        }
    }

    fn found_lfs(&mut self, key: Key, entry: LfsStoreEntry) {
        match entry {
            LfsStoreEntry::PointerAndBlob(ptr, blob) => {
                self.found_attributes(key, LazyFile::Lfs(blob, ptr).into())
            }
            LfsStoreEntry::PointerOnly(ptr) => self.found_pointer(key, ptr, false),
        }
    }

    pub(crate) fn fetch_lfs(&mut self, store: &LfsStore, loc: StoreLocation) {
        let pending = self.pending_storekey(FileAttributes::CONTENT);
        if pending.is_empty() {
            return;
        }

        let fetch_start = std::time::Instant::now();

        debug!(
            "Checking store LFS ({cache}) - Count = {count}",
            cache = match loc {
                StoreLocation::Cache => "cache",
                StoreLocation::Local => "local",
            },
            count = pending.len()
        );

        let mut found = 0;
        let mut found_pointers = 0;
        let mut errors = 0;
        let mut error: Option<String> = None;

        self.metrics.lfs.store(loc).fetch(pending.len());
        for store_key in pending.into_iter() {
            let key = store_key.clone().maybe_into_key().expect(
                "no Key present in StoreKey, even though this should be guaranteed by pending_all",
            );
            match store.fetch_available(&store_key, self.fetch_mode.ignore_result()) {
                Ok(Some(entry)) => {
                    // TODO(meyer): Make found behavior w/r/t LFS pointers and content consistent
                    self.metrics.lfs.store(loc).hit(1);
                    if let LfsStoreEntry::PointerOnly(_) = &entry {
                        found_pointers += 1;
                    } else {
                        found += 1;
                    }
                    self.found_lfs(key, entry)
                }
                Ok(None) => {
                    self.metrics.lfs.store(loc).miss(1);
                }
                Err(err) => {
                    self.metrics.lfs.store(loc).err(1);
                    errors += 1;
                    if error.is_none() {
                        error.replace(format!("{}: {}", key, err));
                    }
                    self.errors.keyed_error(key, err)
                }
            }
        }

        self.metrics
            .lfs
            .store(loc)
            .time_from_duration(fetch_start.elapsed())
            .ok();

        if found != 0 {
            debug!("    Found = {found}", found = found);
        }
        if found_pointers != 0 {
            debug!(
                "    Found Pointers-Only = {found_pointers}",
                found_pointers = found_pointers
            );
        }
        if errors != 0 {
            debug!(
                "    Errors = {errors}, Error = {error:?}",
                errors = errors,
                error = error
            );
        }
    }

    fn found_edenapi(
        entry: FileResponse,
        indexedlog_cache: Option<Arc<IndexedLogHgIdDataStore>>,
        lfs_cache: Option<Arc<LfsStore>>,
        aux_cache: Option<Arc<AuxStore>>,
    ) -> Result<(StoreFile, Option<LfsPointersEntry>)> {
        let entry = entry.result?;

        let key = entry.key.clone();
        let mut file = StoreFile::default();
        let mut lfsptr = None;

        if let Some(aux_data) = entry.aux_data() {
            let aux_data = aux_data.clone();
            if let Some(aux_cache) = aux_cache.as_ref() {
                aux_cache.put(key.hgid, &aux_data)?;
            }
            file.aux_data = Some(aux_data);
        }

        if let Some(content) = entry.content() {
            if content.metadata().is_lfs() {
                let ptr: LfsPointersEntry = entry.try_into()?;
                if let Some(lfs_cache) = lfs_cache.as_ref() {
                    lfs_cache.add_pointer(ptr.clone())?;
                }
                lfsptr = Some(ptr);
            } else if let Some(indexedlog_cache) = indexedlog_cache.as_ref() {
                file.content = Some(Self::evict_to_cache(
                    key,
                    LazyFile::SaplingRemoteApi(entry),
                    indexedlog_cache,
                )?);
            } else {
                file.content = Some(LazyFile::SaplingRemoteApi(entry));
            }
        }

        Ok((file, lfsptr))
    }

    pub(crate) fn fetch_edenapi(
        &mut self,
        store: &SaplingRemoteApiFileStore,
        indexedlog_cache: Option<Arc<IndexedLogHgIdDataStore>>,
        lfs_cache: Option<Arc<LfsStore>>,
        aux_cache: Option<Arc<AuxStore>>,
    ) {
        let fetchable = FileAttributes::CONTENT | FileAttributes::AUX;

        let pending = self.pending_nonlfs(fetchable);
        if pending.is_empty() {
            return;
        }

        let mut fetching_keys: HashSet<Key> = pending.iter().cloned().collect();

        let count = pending.len();
        debug!("Fetching SaplingRemoteAPI - Count = {}", count);

        let mut found = 0;
        let mut found_pointers = 0;
        let mut errors = 0;
        let mut error: Option<String> = None;

        // TODO(meyer): Iterators or otherwise clean this up
        let pending_attrs: Vec<_> = pending
            .into_iter()
            .map(|k| {
                let actionable = self.common.actionable(&k, fetchable, self.compute_aux_data);
                FileSpec {
                    key: k,
                    attrs: actionable.into(),
                }
            })
            .collect();

        // Fetch ClientRequestInfo from a thread local and pass to async code
        let maybe_client_request_info = get_client_request_info_thread_local();
        let response = match block_on(
            with_client_request_info_scope(
                maybe_client_request_info,
                store.files_attrs(pending_attrs),
            )
            .map_err(|e| e.tag_network()),
        ) {
            Ok(r) => r,
            Err(err) => {
                let err = ClonableError::new(err);
                for key in fetching_keys.into_iter() {
                    self.errors.keyed_error(key, err.clone().into());
                }
                return;
            }
        };

        let entries = response
            .entries
            .map(move |res_entry| {
                let lfs_cache = lfs_cache.clone();
                let indexedlog_cache = indexedlog_cache.clone();
                let aux_cache = aux_cache.clone();
                spawn_blocking(move || {
                    res_entry.map(move |entry| {
                        (
                            entry.key.clone(),
                            Self::found_edenapi(entry, indexedlog_cache, lfs_cache, aux_cache),
                        )
                    })
                })

                // Processing a response may involve compressing the response, which
                // can be expensive. If we don't process entries fast enough, edenapi
                // can start queueing up responses which causes forever increasing
                // memory usage. So let's process responses in parallel to stay ahead
                // of download speeds.
            })
            .buffer_unordered(4);

        // Record found entries
        let mut unknown_error: Option<ClonableError> = None;
        for res in stream_to_iter(entries) {
            // TODO(meyer): This outer SaplingRemoteApi error with no key sucks
            let (key, res) = match res {
                Ok(result) => match result.map_err(|e| e.tag_network()) {
                    Ok(result) => result,
                    Err(err) => {
                        if unknown_error.is_none() {
                            unknown_error.replace(ClonableError::new(err));
                        }
                        continue;
                    }
                },
                // JoinError
                Err(err) => {
                    if unknown_error.is_none() {
                        unknown_error.replace(ClonableError::new(err.into()));
                    }
                    continue;
                }
            };

            fetching_keys.remove(&key);
            match res {
                Ok((file, maybe_lfsptr)) => {
                    if let Some(lfsptr) = maybe_lfsptr {
                        found_pointers += 1;
                        self.found_pointer(key.clone(), lfsptr, false);
                    } else {
                        found += 1;
                    }
                    self.found_attributes(key, file);
                }
                Err(err) => {
                    errors += 1;
                    if error.is_none() {
                        error.replace(format!("{}: {}", key, err));
                    }
                    self.errors.keyed_error(key, NetworkError::wrap(err))
                }
            }
        }

        for missing_key in fetching_keys.into_iter() {
            match &unknown_error {
                Some(error) => self.errors.keyed_error(missing_key, error.clone().into()),
                None => {
                    // This should never happen.
                    self.errors.keyed_error(
                        missing_key,
                        anyhow!("key not returned from files_attr request"),
                    )
                }
            };
        }

        if found != 0 {
            debug!("    Found = {found}", found = found);
        }
        if found_pointers != 0 {
            debug!(
                "    Found Pointers = {found_pointers}",
                found_pointers = found_pointers
            );
        }
        if errors != 0 {
            debug!(
                "    Errors = {errors}, Error = {error:?}",
                errors = errors,
                error = error
            );
        }

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

        if let Ok(stats) = block_on(response.stats) {
            util::record_edenapi_stats(&span, &stats);
            // Mononoke already records the time it takes to send the request
            // (from first byte to last byte sent). We are more interested in
            // the total time since it includes time not recorded by Mononoke
            // (routing, cross regional latency, etc).
            self.metrics.edenapi.time_from_duration(stats.time).ok();
        }

        // We subtract any lfs pointers that were found -- these requests were
        // fulfiled by LFS, not SaplingRemoteAPI
        self.metrics.edenapi.fetch(count - found_pointers);
        self.metrics.edenapi.err(errors);
        self.metrics.edenapi.hit(found);
    }

    pub(crate) fn fetch_cas(&mut self, cas_client: &dyn CasClient) {
        let span = tracing::info_span!(
            "fetch_cas",
            keys = field::Empty,
            hits = field::Empty,
            requests = field::Empty,
            time = field::Empty,
        );
        let _enter = span.enter();

        let fetchable = FileAttributes::PURE_CONTENT;

        let mut digest_to_key: HashMap<CasDigest, Key> = self
            // TODO: fetch LFS files
            .pending_nonlfs(fetchable)
            .into_iter()
            // Get AUX data from "pending" (assuming we previously fetched it).
            .filter_map(|key| {
                // TODO: fetch aux data from edenapi on-demand?

                let store_file = self.common.pending.get(&key)?;

                let aux_data = match store_file.aux_data.as_ref() {
                    Some(aux_data) => {
                        tracing::trace!(target: "cas", ?key, ?aux_data, "found aux data for file digest");
                        aux_data
                    }
                    None => {
                        tracing::trace!(target: "cas", ?key, "no aux data for file digest");
                        return None;
                    }
                };

                if self.common.request_attrs.content_header && !store_file.attrs().content_header {
                    // If the caller wants hg content header but the aux data didn't have it,
                    // we won't find it in CAS, so don't bother fetching content from CAS.
                    tracing::trace!(target: "cas", ?key, "no content header in AUX data");
                    None
                } else {
                    Some((
                        CasDigest {
                            hash: aux_data.blake3,
                            size: aux_data.total_size,
                        },
                        key,
                    ))
                }
            })
            .collect();

        if digest_to_key.is_empty() {
            return;
        }

        let digests: Vec<CasDigest> = digest_to_key.keys().cloned().collect();

        span.record("keys", digests.len());

        let mut found = 0;
        let mut error = 0;
        let mut reqs = 0;

        // TODO: configure
        let max_batch_size = 1000;
        let start_time = Instant::now();

        for chunk in digests.chunks(max_batch_size) {
            reqs += 1;

            // TODO: should we fan out here into multiple requests?
            match block_on(cas_client.fetch(chunk, CasDigestType::File)) {
                Ok(results) => {
                    for (digest, data) in results {
                        let Some(key) = digest_to_key.remove(&digest) else {
                            tracing::error!("got CAS result for unrequested digest {:?}", digest);
                            continue;
                        };

                        match data {
                            Err(err) => {
                                tracing::error!(?err, ?key, ?digest, "CAS fetch error");
                                tracing::error!(target: "cas", ?err, ?key, ?digest, "file fetch error");
                                error += 1;
                                self.errors.keyed_error(key, err);
                            }
                            Ok(None) => {
                                tracing::trace!(target: "cas", ?key, ?digest, "file not in cas");
                                // miss
                            }
                            Ok(Some(data)) => {
                                found += 1;
                                tracing::trace!(target: "cas", ?key, ?digest, "file found in cas");
                                self.found_attributes(
                                    key,
                                    StoreFile {
                                        content: Some(LazyFile::Cas(data.into())),
                                        aux_data: None,
                                    },
                                );
                            }
                        }
                    }
                }
                Err(err) => {
                    tracing::error!(?err, "overall CAS error");

                    // Don't propagate CAS error - we want to fall back to SLAPI.
                    error += 1;
                }
            }
        }

        span.record("hits", found);
        span.record("requests", reqs);
        span.record("time", start_time.elapsed().as_millis() as u64);

        let _ = self.metrics.cas.time_from_duration(start_time.elapsed());
        self.metrics.cas.fetch(digests.len());
        self.metrics.cas.err(error);
        self.metrics.cas.hit(found);
    }

    pub(crate) fn fetch_lfs_remote(
        &mut self,
        store: &LfsRemote,
        _local: Option<Arc<LfsStore>>,
        cache: Option<Arc<LfsStore>>,
    ) {
        let errors = &mut self.errors;
        let pending: HashSet<_> = self
            .lfs_pointers
            .iter()
            .map(|(key, (ptr, write))| {
                if *write {
                    if let Some(lfs_cache) = cache.as_ref() {
                        if let Err(err) = lfs_cache.add_pointer(ptr.clone()) {
                            errors.keyed_error(key.clone(), err);
                        }
                    }
                }
                (ptr.sha256(), ptr.size() as usize)
            })
            .collect();

        let mut key_map: HashMap<Sha256, Vec<(Key, LfsPointersEntry)>> = HashMap::new();
        for (key, (ptr, _)) in self.lfs_pointers.iter() {
            let keys = key_map.entry(ptr.sha256()).or_default();
            keys.push((key.clone(), ptr.clone()));
        }

        if pending.is_empty() {
            return;
        }

        debug!("Fetching LFS - Count = {count}", count = pending.len());

        let prog = self.lfs_progress.create_or_extend(pending.len() as u64);

        let mut keyed_errors = Vec::<(Key, anyhow::Error)>::new();
        let mut other_errors = vec![];

        // Fetch & write to local LFS stores
        let top_level_error = store.batch_fetch(
            &pending,
            |sha256, data| -> Result<()> {
                prog.increase_position(1);

                cache
                    .as_ref()
                    .expect("no lfs_cache present when handling cache LFS pointer")
                    .add_blob(&sha256, data.clone())?;

                // Unwrap is safe because the only place sha256 could come from is
                // `pending` and all of its entries were put in `key_map`.
                for (key, ptr) in key_map.get(&sha256).unwrap().iter() {
                    let file = StoreFile {
                        content: Some(LazyFile::Lfs(data.clone(), ptr.clone())),
                        ..Default::default()
                    };

                    self.found_attributes(key.clone(), file);
                    self.lfs_pointers.remove(key);
                }

                Ok(())
            },
            |sha256, error| {
                if let Some(keys) = key_map.get(&sha256) {
                    let error = ClonableError::new(NetworkError::wrap(error));
                    for (key, _) in keys.iter() {
                        keyed_errors.push(((*key).clone(), error.clone().into()));
                    }
                } else {
                    other_errors.push(anyhow!("invalid other lfs error: {:?}", error));
                }
            },
        );

        if let Err(err) = top_level_error {
            let err = ClonableError::new(err);
            for (key, (_ptr, _write)) in self.lfs_pointers.iter() {
                self.errors.keyed_error(key.clone(), err.clone().into());
            }
        }

        for (key, error) in keyed_errors.into_iter() {
            self.errors.keyed_error(key, error);
        }
        for error in other_errors.into_iter() {
            self.errors.other_error(error);
        }
    }

    // TODO(meyer): Improve how local caching works. At the very least do this in the background.
    // TODO(meyer): Log errors here instead of just ignoring.
    pub(crate) fn derive_computable(&mut self, aux_cache: Option<&AuxStore>) {
        if !self.compute_aux_data {
            return;
        }

        // When ignoring results, we don't reliably have file content, so don't derive.
        if self.fetch_mode.ignore_result() {
            return;
        }

        self.common.pending.retain(|key, value| {
            let span = tracing::debug_span!("checking derivations", %key);
            let _guard = span.enter();

            let existing_attrs = value.attrs();
            let missing = self.common.request_attrs - existing_attrs;
            let actionable = existing_attrs.with_computable() & missing;

            if actionable.aux_data {
                let mut new = std::mem::take(value);

                tracing::debug!("computing aux data");
                if let Err(err) = new.compute_aux_data() {
                    self.errors.keyed_error(key.clone(), err);
                } else {
                    tracing::debug!("computed aux data");

                    // mark complete if applicable
                    if new.attrs().has(self.common.request_attrs) {
                        tracing::debug!("marking complete");

                        self.metrics.aux.store(StoreLocation::Cache).computed(1);

                        if let Some(aux_cache) = aux_cache {
                            if let Some(ref aux_data) = new.aux_data {
                                let _ = aux_cache.put(key.hgid, aux_data);
                            }
                        }

                        let new = new.mask(self.common.request_attrs);

                        if !self.fetch_mode.ignore_result() {
                            let _ = self.common.found_tx.send(Ok((key.clone(), new)));
                        }

                        // Remove this entry from `pending`.
                        return false;
                    } else {
                        *value = new;
                    }
                }
            }

            // Don't remove this entry from `pending`.
            true
        });
    }

    pub(crate) fn finish(self) {
        self.common.results(self.errors);
    }
}
