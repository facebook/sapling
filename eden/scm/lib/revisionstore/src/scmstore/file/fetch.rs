/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::future;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use anyhow::anyhow;
use async_runtime::block_on;
use async_runtime::spawn_blocking;
use async_runtime::stream_to_iter;
use blob::Blob;
use cas_client::CasClient;
use edenapi_types::FileResponse;
use edenapi_types::FileSpec;
use flume::Sender;
use futures::StreamExt;
use futures::TryFutureExt;
use minibytes::Bytes;
use progress_model::ProgressBar;
use storemodel::SerializationFormat;
use tracing::debug;
use tracing::field;
use types::CasDigest;
use types::CasDigestType;
use types::CasFetchedStats;
use types::FetchContext;
use types::HgId;
use types::Key;
use types::Sha256;
use types::errors::NetworkError;

use super::metrics;
use crate::ContentHash;
use crate::Metadata;
use crate::SaplingRemoteApiFileStore;
use crate::StoreKey;
use crate::error::ClonableError;
use crate::indexedlogauxstore::AuxStore;
use crate::indexedlogdatastore::Entry;
use crate::indexedlogdatastore::IndexedLogHgIdDataStore;
use crate::lfs::LfsClient;
use crate::lfs::LfsPointersEntry;
use crate::lfs::LfsStore;
use crate::lfs::LfsStoreEntry;
use crate::scmstore::FileAttributes;
use crate::scmstore::FileAuxData;
use crate::scmstore::FileStore;
use crate::scmstore::StoreFile;
use crate::scmstore::attrs::StoreAttrs;
use crate::scmstore::fetch::CommonFetchState;
use crate::scmstore::fetch::FetchErrors;
use crate::scmstore::fetch::KeyFetchError;
use crate::scmstore::file::LazyFile;
use crate::scmstore::file::metrics::FileStoreFetchMetrics;
use crate::scmstore::metrics::StoreLocation;
use crate::scmstore::value::StoreValue;
use crate::util;

pub struct FetchState {
    common: CommonFetchState<StoreFile>,

    /// Errors encountered during fetching.
    errors: FetchErrors,

    /// LFS pointers we've discovered corresponding to a request Key.
    lfs_pointers: HashMap<Key, (LfsPointersEntry, bool)>,

    /// Track fetch metrics,
    metrics: &'static FileStoreFetchMetrics,

    // Config
    compute_aux_data: bool,

    lfs_enabled: bool,

    fctx: FetchContext,

    format: SerializationFormat,

    cas_cache_threshold_bytes: Option<u64>,

    file_cache: Option<Arc<IndexedLogHgIdDataStore>>,
    files_to_cache: Vec<(HgId, Entry)>,
}

impl FetchState {
    pub(crate) fn new(
        keys: impl IntoIterator<Item = Key>,
        attrs: FileAttributes,
        file_store: &FileStore,
        found_tx: Sender<Result<(Key, StoreFile), KeyFetchError>>,
        lfs_enabled: bool,
        fctx: FetchContext,
        cas_cache_threshold_bytes: Option<u64>,
        bar: Arc<ProgressBar>,
        file_cache: Option<Arc<IndexedLogHgIdDataStore>>,
    ) -> Self {
        FetchState {
            common: CommonFetchState::new(keys, attrs, found_tx, fctx.clone(), bar),
            errors: FetchErrors::new(),
            metrics: if fctx.cause().is_prefetch() {
                &metrics::FILE_STORE_PREFETCH_METRICS
            } else {
                &metrics::FILE_STORE_FETCH_METRICS
            },

            lfs_pointers: HashMap::new(),

            compute_aux_data: file_store.compute_aux_data,
            lfs_enabled,
            format: file_store.format(),
            fctx,
            cas_cache_threshold_bytes,
            file_cache,
            files_to_cache: Vec::new(),
        }
    }

    pub(crate) fn pending_len(&self) -> usize {
        self.common.pending_len()
    }

    pub(crate) fn all_keys(&self) -> Vec<Key> {
        self.common.all_keys()
    }

    pub(crate) fn format(&self) -> SerializationFormat {
        self.format
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

    fn cache_entry(&mut self, entry: Entry) {
        self.files_to_cache.push((entry.node(), entry));
        if self.files_to_cache.len() >= 1_000 {
            self.flush_to_indexedlog();
        }
    }

    pub(crate) fn fetch_indexedlog(&mut self, store: &IndexedLogHgIdDataStore, loc: StoreLocation) {
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

        self.metrics.indexedlog.store(loc).fetch(pending.len());
        let format = self.format();

        let bar = ProgressBar::new_adhoc("IndexedLog", 0, "files");

        self.common
            .iter_pending(FileAttributes::CONTENT, self.compute_aux_data, |key| {
                count += 1;
                bar.increase_position(1);

                let res = if self.fctx.mode().ignore_result() {
                    store.contains(&key.hgid).map(|contains| {
                        if contains {
                            // Insert a stub entry if caller is ignoring the results.
                            Some(Entry::new(key.hgid, Bytes::new(), Metadata::default()))
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
                            return None;
                        } else {
                            return Some(LazyFile::IndexedLog(entry, format).into());
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
        let ignore_results = self.fctx.mode().ignore_result() && !have_cas;

        let mut wants_aux = FileAttributes::AUX;
        if have_cas && loc == StoreLocation::Cache {
            // Also fetch AUX data if we are going to try fetching from CAS. This does two things:
            // 1. Fetches hash and size info needed to query CAS for file contents.
            // 2. Fetches hg content header, which is not available from CAS.
            wants_aux |= FileAttributes::PURE_CONTENT;
        }

        // If we are querying for content header without content, that can be satisfied
        // purely from AUX. Otherwise, don't say AUX can satisfy CONTENT_HEADER (to avoid
        // querying AUX unnecessarily when the header will come with the content).
        if self.common.request_attrs.content_header && !self.common.request_attrs.pure_content {
            wants_aux |= FileAttributes::CONTENT_HEADER;
        }

        let bar = ProgressBar::new_adhoc("IndexedLog", 0, "file metadata");

        self.common
            .iter_pending(wants_aux, self.compute_aux_data, |key| {
                count += 1;
                bar.increase_position(1);

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
                    store.get(&key.hgid)
                };
                match res {
                    Ok(Some(aux)) => {
                        if have_cas {
                            tracing::trace!(target: "cas_client", ?key, ?aux, "found file aux data");
                        }
                        self.metrics.aux.store(loc).hit(1);
                        found += 1;
                        return Some(aux.into());
                    }
                    Ok(None) => {
                        if have_cas {
                            tracing::trace!(target: "cas_client", ?key, "no file aux data");
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
                self.found_attributes(key, LazyFile::Lfs(blob, ptr, self.format).into())
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

        let bar = ProgressBar::new_adhoc("LFS cache", pending.len() as u64, "files");

        self.metrics.lfs.store(loc).fetch(pending.len());
        for store_key in pending.into_iter() {
            let key = store_key.clone().maybe_into_key().expect(
                "no Key present in StoreKey, even though this should be guaranteed by pending_all",
            );
            match store.fetch_available(&store_key, self.fctx.mode().ignore_result()) {
                Ok(Some(entry)) => {
                    bar.increase_position(1);
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
                    bar.increase_position(1);
                    self.metrics.lfs.store(loc).miss(1);
                }
                Err(err) => {
                    bar.increase_position(1);
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
        format: SerializationFormat,
    ) -> Result<(StoreFile, Option<LfsPointersEntry>, Option<Entry>)> {
        let entry = entry.result?;

        let hgid = entry.key.hgid;
        let mut file = StoreFile::default();
        let mut lfsptr = None;
        let mut cache_entry = None;

        if let Some(aux_data) = entry.aux_data() {
            let aux_data = aux_data.clone();
            if let Some(aux_cache) = aux_cache.as_ref() {
                aux_cache.put(hgid, &aux_data)?;
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
            } else {
                if let Some(cache) = &indexedlog_cache {
                    let mut e = Entry::new(hgid, entry.data()?, entry.metadata()?.clone());

                    // Pre-compress content here since we are being called in parallel (and
                    // compression is CPU intensive).
                    cache.maybe_compress_content(&mut e)?;

                    cache_entry = Some(e);
                }

                file.content = Some(LazyFile::SaplingRemoteApi(entry, format));
            }
        }

        Ok((file, lfsptr, cache_entry))
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

        let bar = ProgressBar::new_adhoc("SLAPI", pending_attrs.len() as u64, "files");

        let response = match block_on(
            store
                .files_attrs(self.fctx.clone(), pending_attrs)
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

        let format = self.format();
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
                            Self::found_edenapi(
                                entry,
                                indexedlog_cache,
                                lfs_cache,
                                aux_cache,
                                format,
                            ),
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
            bar.increase_position(1);

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
                Ok((mut file, maybe_lfsptr, cache_entry)) => {
                    if let Some(lfsptr) = maybe_lfsptr {
                        found_pointers += 1;
                        self.found_pointer(key.clone(), lfsptr, false);
                    } else {
                        found += 1;

                        if self.fctx.mode().ignore_result() {
                            // If caller doesn't care about content, swap to a stub file to avoid
                            // needlessly shuffling data around.
                            file = StoreFile {
                                // We obviously aren't Cas, but this is the simplest LazyFile variant to stub.
                                content: Some(LazyFile::Cas(Blob::Bytes(minibytes::Bytes::new()))),
                                aux_data: file.aux_data,
                            }
                        }
                    }
                    if let Some(cache_entry) = cache_entry {
                        self.cache_entry(cache_entry);
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

        // Flush buffered entries to indexedlog so they are visible to other readers sooner (i.e.
        // don't wait until after LFS fetching is done). This appends to the indexedlog in-memory
        // buffer, but will not necessarily sync the logs to disk.
        self.flush_to_indexedlog();

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
        // fulfilled by LFS, not SaplingRemoteAPI
        self.metrics.edenapi.fetch(count - found_pointers);
        self.metrics.edenapi.err(errors);
        self.metrics.edenapi.hit(found);
    }

    pub(crate) fn fetch_cas(&mut self, cas_client: &dyn CasClient) {
        self.fctx.set_fetch_from_cas_attempted(true);

        if self.common.request_attrs == FileAttributes::AUX {
            // If we are only requesting aux data, don't bother querying CAS. Aux data is
            // required to query CAS, so CAS cannot possibly help.
            return;
        }

        let span = tracing::info_span!(
            "fetch_cas",
            keys = field::Empty,
            hits = field::Empty,
            requests = field::Empty,
            time = field::Empty,
        );
        let _enter = span.enter();

        let fetchable = FileAttributes::PURE_CONTENT;

        let bar = ProgressBar::new_adhoc("CAS", 0, "digests");

        let digest_with_keys: Vec<(CasDigest, Key)> = self
            // TODO: fetch LFS files
            .pending_nonlfs(fetchable)
            .into_iter()
            // Get AUX data from "pending" (assuming we previously fetched it).
            .filter_map(|key| {
                // TODO: fetch aux data from edenapi on-demand?

                let store_file = self.common.pending.get(&key)?;

                let aux_data = match store_file.aux_data.as_ref() {
                    Some(aux_data) => {
                        tracing::trace!(target: "cas_client", ?key, ?aux_data, "found aux data for file digest");
                        aux_data
                    }
                    None => {
                        tracing::trace!(target: "cas_client", ?key, "no aux data for file digest");
                        return None;
                    }
                };

                if let Some(cas_threshold) = self.cas_cache_threshold_bytes {
                    if aux_data.total_size > cas_threshold {
                        // If the file's size exceeds the configured threshold, don't fetch it from CAS.
                        return None;
                    }
                }

                bar.increase_position(1);

                if self.common.request_attrs.content_header && !store_file.attrs().content_header {
                    // If the caller wants hg content header but the aux data didn't have it,
                    // we won't find it in CAS, so don't bother fetching content from CAS.
                    tracing::trace!(target: "cas_client", ?key, "no content header in AUX data");
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

        drop(bar);

        // Include the duplicates in the count.
        let keys_fetch_count = digest_with_keys.len();

        span.record("keys", keys_fetch_count);

        let mut digest_to_key: HashMap<CasDigest, Vec<Key>> = HashMap::default();

        for (digest, key) in digest_with_keys {
            digest_to_key.entry(digest).or_default().push(key);
        }

        if digest_to_key.is_empty() {
            return;
        }

        let digests: Vec<CasDigest> = digest_to_key.keys().cloned().collect();

        let mut keys_found_count = 0;
        let mut error = 0;
        let mut reqs = 0;

        let start_time = Instant::now();
        let mut total_stats = CasFetchedStats::default();

        let bar = ProgressBar::new_adhoc("CAS", digests.len() as u64, "files");

        if self.fctx.mode().ignore_result() {
            // Prefetching files, so we don't need the data, just to ensure digests are in the CAS local Cache.
            block_on(async {
                cas_client.prefetch(self.fctx.clone(), &digests, CasDigestType::File).await.for_each(|results| {
                    match results {
                    Ok((stats, digests_prefetched, digests_not_found)) => {
                        reqs += 1;
                        total_stats.add(&stats);
                        if !digests_not_found.is_empty() {
                            tracing::trace!(target: "cas_client", "{} digests are missing in CAS", digests_not_found.len());
                        }
                        for digest in digests_prefetched {
                            bar.increase_position(1);

                            let Some(keys) = digest_to_key.remove(&digest) else {
                                tracing::error!("got CAS result for unrequested digest {:?}", digest);
                                continue;
                            };
                            keys_found_count += keys.len();
                            tracing::trace!(target: "cas_client", ?keys, ?digest, "file(s) prefetched in cas");
                            for key in keys {
                                self.found_attributes(
                                    key,
                                    StoreFile {
                                        content: Some(LazyFile::Cas(Blob::Bytes(minibytes::Bytes::new()))),
                                        aux_data: None,
                                    },
                                );
                            }
                        }
                        future::ready(())
                    }
                    Err(err) => {
                        tracing::error!(?err, "overall CAS error");
                        tracing::error!(target: "cas_client", ?err, "CAS error prefetching files");
                        // Don't propagate CAS error - we want to fall back to SLAPI.
                        reqs += 1;
                        error += 1;
                        future::ready(())
                    }
                }}).await;
            });
        } else {
            // Fetching files, we need the data.
            block_on(async {
                cas_client.fetch(self.fctx.clone(), &digests, CasDigestType::File).await.for_each(|results|{
                    match results {
                    Ok((stats, results)) => {
                        reqs += 1;
                        total_stats.add(&stats);
                        for (digest, data) in results {
                            bar.increase_position(1);

                            let Some(mut keys) = digest_to_key.remove(&digest) else {
                                tracing::error!("got CAS result for unrequested digest {:?}", digest);
                                continue;
                            };
                            match data {
                                Err(err) => {
                                    tracing::error!(?err, ?keys, ?digest, "CAS fetch error");
                                    tracing::error!(target: "cas_client", ?err, ?keys, ?digest, "file(s) fetch error");
                                    error += keys.len();
                                    self.errors.multiple_keyed_error(keys, "CAS fetch error", err);
                                }
                                Ok(None) => {
                                    tracing::trace!(target: "cas_client", ?keys, ?digest, "file(s) not in cas");
                                    // miss
                                }
                                Ok(Some(data)) => {
                                    keys_found_count += keys.len();
                                    tracing::trace!(target: "cas_client", ?keys, ?digest, "file(s) found in cas");
                                    if !keys.is_empty() {
                                        let last = keys.pop().unwrap();
                                        for key in keys {
                                            self.found_attributes(
                                                key,
                                                StoreFile {
                                                    content: Some(LazyFile::Cas(data.clone())),
                                                    aux_data: None,
                                                },
                                            );
                                        }
                                        // zero copy here
                                        self.found_attributes(
                                            last,
                                            StoreFile {
                                                content: Some(LazyFile::Cas(data)),
                                                aux_data: None,
                                            },
                                        );
                                    }
                                }
                            }
                        }
                        future::ready(())
                    }
                    Err(err) => {
                        tracing::error!(?err, "overall CAS error");
                        tracing::error!(target: "cas_client", ?err, "CAS error fetching files");

                        // Don't propagate CAS error - we want to fall back to SLAPI.
                        reqs += 1;
                        error += 1;
                        future::ready(())
                    }
                }}).await;
            });
        }

        span.record("hits", keys_found_count);
        span.record("requests", reqs);
        span.record("time", start_time.elapsed().as_millis() as u64);

        let _ = self.metrics.cas.time_from_duration(start_time.elapsed());
        self.metrics.cas.fetch(keys_fetch_count);
        self.metrics.cas.err(error);
        self.metrics.cas.hit(keys_found_count);
        self.metrics.cas.miss(keys_fetch_count - keys_found_count);
        self.metrics.cas_backend.update(&total_stats);
        self.metrics.cas_local_cache.update(&total_stats);
    }

    pub(crate) fn fetch_lfs_remote(&mut self, client: &LfsClient, buffer_in_memory: bool) {
        let cache = &client.shared;

        let errors = &mut self.errors;
        let pending: HashSet<_> = self
            .lfs_pointers
            .iter()
            .map(|(key, (ptr, write))| {
                if *write {
                    if let Err(err) = cache.add_pointer(ptr.clone()) {
                        errors.keyed_error(key.clone(), err);
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

        let mut keyed_errors = Vec::<(Key, anyhow::Error)>::new();
        let mut other_errors = vec![];

        let bar = ProgressBar::new_adhoc("LFS remote", pending.len() as u64, "files");

        // Fetch & write to local LFS stores
        let top_level_error = if buffer_in_memory {
            // Legacy path that streams LFS blob into memory.
            client.remote.batch_fetch(
                self.fctx.clone(),
                &pending,
                |sha256, data| -> Result<()> {
                    bar.increase_position(1);

                    cache.add_blob(&sha256, data.clone())?;

                    // Unwrap is safe because the only place sha256 could come from is
                    // `pending` and all of its entries were put in `key_map`.
                    for (key, ptr) in key_map.get(&sha256).unwrap().iter() {
                        let file = StoreFile {
                            content: Some(LazyFile::Lfs(
                                data.clone().into(),
                                ptr.clone(),
                                self.format,
                            )),
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
            )
        } else {
            // Put LFS cache into "consistent read" mode where cache insertions we made during
            // `batch_fetch` are guaranteed to be readable from cache. This is so we can download,
            // flush cache, and return mmap backed slices without worrying about the cache rotating
            // out from underneath us.
            let _consistent_reads = cache.with_consistent_reads();

            let mut successful_hashes = Vec::with_capacity(pending.len());

            // New path that streams LFS blob into indexedlog cache.
            let res = client.batch_fetch(
                self.fctx.clone(),
                &pending,
                |sha256| {
                    bar.increase_position(1);
                    successful_hashes.push(sha256);
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

            if !self.fctx.mode().ignore_result() {
                // Sync buffered chunks to indexedlog so the fetching below hits the mmap'd back data.
                // The goal is to never hold an entire LFS file's content on the heap.
                if let Err(err) = cache.flush() {
                    self.errors.other_error(err);
                }
            }

            // Unwrap is safe because the only place sha256 could come from is
            // `pending` and all of its entries were put in `key_map`.
            for hash in successful_hashes {
                for (key, ptr) in key_map.get(&hash).unwrap().iter() {
                    let file = if self.fctx.mode().ignore_result() {
                        // Caller doesn't want data - send stub value.
                        StoreFile {
                            content: Some(LazyFile::Lfs(
                                Bytes::new().into(),
                                ptr.clone(),
                                self.format,
                            )),
                            ..Default::default()
                        }
                    } else {
                        let data = match cache.get_blob(&hash, ptr.size()) {
                            Ok(Some(data)) => data,
                            Ok(None) => {
                                self.errors.keyed_error(
                                    key.clone(),
                                    anyhow!("LFS file missing from cache after download"),
                                );
                                continue;
                            }
                            Err(err) => {
                                self.errors.keyed_error(key.clone(), err);
                                continue;
                            }
                        };

                        StoreFile {
                            content: Some(LazyFile::Lfs(data, ptr.clone(), self.format)),
                            ..Default::default()
                        }
                    };

                    self.found_attributes(key.clone(), file);
                    self.lfs_pointers.remove(key);
                }
            }

            res
        };

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
        if self.fctx.mode().ignore_result() {
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

                        if !self.fctx.mode().ignore_result() {
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
}

impl FetchState {
    // Flush pending files to indexedlog file cache. This appends to the indexedlog in-memory
    // buffer, but will not necessarily sync the logs to disk.
    fn flush_to_indexedlog(&mut self) {
        if let Some(file_cache) = &self.file_cache {
            if let Err(err) = file_cache.put_batch(std::mem::take(&mut self.files_to_cache)) {
                self.errors.other_error(err);
            }
        }
    }

    pub(crate) fn finish(&mut self) {
        // We made it to the end with no overall errors - report_mising=true so we report errors
        // for any items we unexpectedly didn't get results for.
        self.common.results(std::mem::take(&mut self.errors), true);
    }
}

impl Drop for FetchState {
    fn drop(&mut self) {
        self.flush_to_indexedlog();

        self.common.results(std::mem::take(&mut self.errors), false);
    }
}
