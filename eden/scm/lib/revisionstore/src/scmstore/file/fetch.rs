/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use async_runtime::block_on;
use async_runtime::spawn_blocking;
use async_runtime::stream_to_iter;
use clientinfo::get_client_request_info_thread_local;
use clientinfo_async::with_client_request_info_scope;
use crossbeam::channel::Sender;
use edenapi_types::FileResponse;
use edenapi_types::FileSpec;
use futures::StreamExt;
use futures::TryFutureExt;
use progress_model::AggregatingProgressBar;
use tracing::debug;
use tracing::field;
use types::errors::NetworkError;
use types::Key;
use types::Sha256;

use crate::datastore::HgIdDataStore;
use crate::datastore::RemoteDataStore;
use crate::error::ClonableError;
use crate::fetch_logger::FetchLogger;
use crate::indexedlogauxstore::AuxStore;
use crate::indexedlogauxstore::Entry as AuxDataEntry;
use crate::indexedlogdatastore::Entry;
use crate::indexedlogdatastore::IndexedLogHgIdDataStore;
use crate::indexedlogutil::StoreType;
use crate::lfs::LfsPointersEntry;
use crate::lfs::LfsRemoteInner;
use crate::lfs::LfsStore;
use crate::lfs::LfsStoreEntry;
use crate::scmstore::attrs::StoreAttrs;
use crate::scmstore::fetch::CommonFetchState;
use crate::scmstore::fetch::FetchErrors;
use crate::scmstore::fetch::KeyFetchError;
use crate::scmstore::file::metrics::FileStoreFetchMetrics;
use crate::scmstore::file::LazyFile;
use crate::scmstore::value::StoreValue;
use crate::scmstore::FetchMode;
use crate::scmstore::FileAttributes;
use crate::scmstore::FileAuxData;
use crate::scmstore::FileStore;
use crate::scmstore::StoreFile;
use crate::util;
use crate::ContentHash;
use crate::ContentStore;
use crate::EdenApiFileStore;
use crate::ExtStoredPolicy;
use crate::Metadata;
use crate::StoreKey;

pub struct FetchState {
    common: CommonFetchState<StoreFile>,

    /// Errors encountered during fetching.
    errors: FetchErrors,

    /// LFS pointers we've discovered corresponding to a request Key.
    lfs_pointers: HashMap<Key, (LfsPointersEntry, bool)>,

    /// A table tracking if discovered LFS pointers were found in the local-only or cache / shared store.
    pointer_origin: HashMap<Sha256, StoreType>,

    /// A table tracking if each key is local-only or cache/shared so that computed aux data can be written to the appropriate store
    key_origin: HashMap<Key, StoreType>,

    /// Tracks remote fetches which match a specific regex
    fetch_logger: Option<Arc<FetchLogger>>,

    lfs_progress: Arc<AggregatingProgressBar>,

    /// Track fetch metrics,
    metrics: FileStoreFetchMetrics,

    // Config
    extstored_policy: ExtStoredPolicy,
    compute_aux_data: bool,

    lfs_enabled: bool,
}

impl FetchState {
    pub(crate) fn new(
        keys: impl Iterator<Item = Key>,
        attrs: FileAttributes,
        file_store: &FileStore,
        found_tx: Sender<Result<(Key, StoreFile), KeyFetchError>>,
        lfs_enabled: bool,
        mode: FetchMode,
    ) -> Self {
        FetchState {
            common: CommonFetchState::new(keys, attrs, found_tx, mode),
            errors: FetchErrors::new(),
            metrics: FileStoreFetchMetrics::default(),

            lfs_pointers: HashMap::new(),
            key_origin: HashMap::new(),
            pointer_origin: HashMap::new(),

            fetch_logger: file_store.fetch_logger.clone(),
            extstored_policy: file_store.extstored_policy,
            compute_aux_data: true,
            lfs_progress: file_store.lfs_progress.clone(),
            lfs_enabled,
        }
    }

    pub(crate) fn pending_len(&self) -> usize {
        self.common.pending_len()
    }

    pub(crate) fn pending(&self) -> Vec<Key> {
        self.common.pending.iter().cloned().collect()
    }

    pub(crate) fn metrics(&self) -> &FileStoreFetchMetrics {
        &self.metrics
    }

    /// Return all incomplete requested Keys for which additional attributes may be gathered by querying a store which provides the specified attributes.
    fn pending_all(&self, fetchable: FileAttributes) -> Vec<Key> {
        if fetchable.none() {
            return vec![];
        }
        self.common
            .pending(fetchable, self.compute_aux_data)
            .map(|(key, _attrs)| key.clone())
            .collect()
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

    fn mark_complete(&mut self, key: &Key) {
        if let Some((ptr, _)) = self.lfs_pointers.remove(key) {
            self.pointer_origin.remove(&ptr.sha256());
        }
    }

    fn found_pointer(&mut self, key: Key, ptr: LfsPointersEntry, typ: StoreType, write: bool) {
        let sha256 = ptr.sha256();
        // Overwrite StoreType::Local with StoreType::Shared, but not vice versa
        match typ {
            StoreType::Shared => {
                self.pointer_origin.insert(sha256, typ);
            }
            StoreType::Local => {
                self.pointer_origin.entry(sha256).or_insert(typ);
            }
        }
        self.lfs_pointers.insert(key, (ptr, write));
    }

    fn found_attributes(&mut self, key: Key, sf: StoreFile, typ: Option<StoreType>) {
        self.key_origin
            .insert(key.clone(), typ.unwrap_or(StoreType::Shared));

        if self.common.found(key.clone(), sf) {
            self.mark_complete(&key);
        }
    }

    fn evict_to_cache(
        key: Key,
        file: LazyFile,
        indexedlog_cache: &IndexedLogHgIdDataStore,
    ) -> Result<LazyFile> {
        let cache_entry = file.indexedlog_cache_entry(key.clone())?.ok_or_else(|| {
            anyhow!(
                "expected LazyFile::EdenApi, other LazyFile variants should not be written to cache"
            )
        })?;
        indexedlog_cache.put_entry(cache_entry)?;
        let mmap_entry = indexedlog_cache
            .get_entry(key)?
            .ok_or_else(|| anyhow!("failed to read entry back from indexedlog after writing"))?;
        Ok(LazyFile::IndexedLog(mmap_entry))
    }

    fn found_indexedlog(
        &mut self,
        key: Key,
        entry: Entry,
        typ: StoreType,
        lfs_store: Option<&LfsStore>,
    ) {
        if entry.metadata().is_lfs() && self.lfs_enabled {
            if self.extstored_policy == ExtStoredPolicy::Use {
                match entry.try_into() {
                    Ok(ptr) => {
                        if let Some(lfs_store) = lfs_store {
                            // Promote this indexedlog LFS pointer to the
                            // pointer store if it isn't already present. This
                            // should only happen when the Python LFS extension
                            // is in play.
                            if let Ok(None) = lfs_store.fetch_available(&key.clone().into()) {
                                if let Err(err) = lfs_store.add_pointer(ptr) {
                                    self.errors.keyed_error(key, err);
                                }
                            }
                        } else {
                            // If we don't have somewhere to upgrade pointer,
                            // track as a "found" pointer so it will be fetched
                            // from the remote store subsequently.
                            self.found_pointer(key, ptr, typ, true)
                        }
                    }
                    Err(err) => self.errors.keyed_error(key, err),
                }
            }
        } else {
            self.found_attributes(key, LazyFile::IndexedLog(entry).into(), Some(typ))
        }
    }

    pub(crate) fn fetch_indexedlog(
        &mut self,
        store: &IndexedLogHgIdDataStore,
        lfs_store: Option<&LfsStore>,
        typ: StoreType,
    ) {
        let pending = self.pending_nonlfs(FileAttributes::CONTENT);
        if pending.is_empty() {
            return;
        }

        let fetch_start = std::time::Instant::now();

        debug!(
            "Checking store Indexedlog ({cache}) for {key}{more}",
            cache = match typ {
                StoreType::Shared => "cache",
                StoreType::Local => "local",
            },
            key = pending[0],
            more = if pending.len() > 1 {
                format!(" and {} more", pending.len() - 1)
            } else {
                "".into()
            },
        );

        let mut found = 0;
        let mut errors = 0;
        let mut error: Option<String> = None;

        self.metrics.indexedlog.store(typ).fetch(pending.len());
        for key in pending.into_iter() {
            let res = store.get_raw_entry(&key.hgid);
            match res {
                Ok(Some(entry)) => {
                    self.metrics.indexedlog.store(typ).hit(1);
                    found += 1;
                    self.found_indexedlog(key, entry, typ, lfs_store)
                }
                Ok(None) => {
                    self.metrics.indexedlog.store(typ).miss(1);
                }
                Err(err) => {
                    self.metrics.indexedlog.store(typ).err(1);
                    errors += 1;
                    if error.is_none() {
                        error.replace(format!("{}: {}", key, err));
                    }
                    self.errors.keyed_error(key, err)
                }
            }
        }

        self.metrics
            .indexedlog
            .store(typ)
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

    fn found_aux_indexedlog(&mut self, key: Key, entry: AuxDataEntry, typ: StoreType) {
        let aux_data: FileAuxData = entry.into();
        self.found_attributes(key, aux_data.into(), Some(typ));
    }

    pub(crate) fn fetch_aux_indexedlog(&mut self, store: &AuxStore, typ: StoreType) {
        let pending = self.pending_all(FileAttributes::AUX);
        if pending.is_empty() {
            return;
        }

        let fetch_start = std::time::Instant::now();

        debug!(
            "Checking store AUX ({cache}) - Count = {count}",
            cache = match typ {
                StoreType::Shared => "cache",
                StoreType::Local => "local",
            },
            count = pending.len()
        );

        let mut found = 0;
        let mut errors = 0;
        let mut error: Option<String> = None;

        self.metrics.aux.store(typ).fetch(pending.len());

        for key in pending.into_iter() {
            let res = store.get(key.hgid);
            match res {
                Ok(Some(aux)) => {
                    self.metrics.aux.store(typ).hit(1);
                    found += 1;
                    self.found_aux_indexedlog(key, aux, typ)
                }
                Ok(None) => {
                    self.metrics.aux.store(typ).miss(1);
                }
                Err(err) => {
                    self.metrics.aux.store(typ).err(1);
                    errors += 1;
                    if error.is_none() {
                        error.replace(format!("{}: {}", key, err));
                    }
                    self.errors.keyed_error(key, err)
                }
            }
        }

        self.metrics
            .aux
            .store(typ)
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

    fn found_lfs(&mut self, key: Key, entry: LfsStoreEntry, typ: StoreType) {
        match entry {
            LfsStoreEntry::PointerAndBlob(ptr, blob) => {
                self.found_attributes(key, LazyFile::Lfs(blob, ptr).into(), Some(typ))
            }
            LfsStoreEntry::PointerOnly(ptr) => self.found_pointer(key, ptr, typ, false),
        }
    }

    pub(crate) fn fetch_lfs(&mut self, store: &LfsStore, typ: StoreType) {
        let pending = self.pending_storekey(FileAttributes::CONTENT);
        if pending.is_empty() {
            return;
        }

        let fetch_start = std::time::Instant::now();

        debug!(
            "Checking store LFS ({cache}) - Count = {count}",
            cache = match typ {
                StoreType::Shared => "cache",
                StoreType::Local => "local",
            },
            count = pending.len()
        );

        let mut found = 0;
        let mut found_pointers = 0;
        let mut errors = 0;
        let mut error: Option<String> = None;

        self.metrics.lfs.store(typ).fetch(pending.len());
        for store_key in pending.into_iter() {
            let key = store_key.clone().maybe_into_key().expect(
                "no Key present in StoreKey, even though this should be guaranteed by pending_all",
            );
            match store.fetch_available(&store_key) {
                Ok(Some(entry)) => {
                    // TODO(meyer): Make found behavior w/r/t LFS pointers and content consistent
                    self.metrics.lfs.store(typ).hit(1);
                    if let LfsStoreEntry::PointerOnly(_) = &entry {
                        found_pointers += 1;
                    } else {
                        found += 1;
                    }
                    self.found_lfs(key, entry, typ)
                }
                Ok(None) => {
                    self.metrics.lfs.store(typ).miss(1);
                }
                Err(err) => {
                    self.metrics.lfs.store(typ).err(1);
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
            .store(typ)
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
            let aux_data: FileAuxData = aux_data.clone().into();
            if let Some(aux_cache) = aux_cache.as_ref() {
                aux_cache.put(key.hgid, &aux_data.into())?;
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
                    LazyFile::EdenApi(entry),
                    indexedlog_cache,
                )?);
            } else {
                file.content = Some(LazyFile::EdenApi(entry));
            }
        }

        Ok((file, lfsptr))
    }

    pub(crate) fn fetch_edenapi(
        &mut self,
        store: &EdenApiFileStore,
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
        debug!("Fetching EdenAPI - Count = {}", count);

        let mut found = 0;
        let mut found_pointers = 0;
        let mut errors = 0;
        let mut error: Option<String> = None;

        if let Some(fl) = self.fetch_logger.as_ref() {
            fl.report_keys(pending.iter())
        }

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
            // TODO(meyer): This outer EdenApi error with no key sucks
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
                        self.found_pointer(key.clone(), lfsptr, StoreType::Shared, false);
                    } else {
                        found += 1;
                    }
                    self.found_attributes(key, file, Some(StoreType::Shared));
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
        // fulfiled by LFS, not EdenAPI
        self.metrics.edenapi.fetch(count - found_pointers);
        self.metrics.edenapi.err(errors);
        self.metrics.edenapi.hit(found);
    }

    pub(crate) fn fetch_lfs_remote(
        &mut self,
        store: &LfsRemoteInner,
        local: Option<Arc<LfsStore>>,
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

        if let Some(fl) = self.fetch_logger.as_ref() {
            fl.report_keys(self.lfs_pointers.keys())
        }

        let prog = self.lfs_progress.create_or_extend(pending.len() as u64);

        let mut keyed_errors = Vec::<(Key, anyhow::Error)>::new();
        let mut other_errors = vec![];

        // Fetch & write to local LFS stores
        let top_level_error = store.batch_fetch(
            &pending,
            |sha256, data| -> Result<()> {
                prog.increase_position(1);

                match self.pointer_origin.get(&sha256).ok_or_else(|| {
                    anyhow!(
                        "no source found for Sha256; received unexpected Sha256 from LFS server"
                    )
                })? {
                    StoreType::Local => local
                        .as_ref()
                        .expect("no lfs_local present when handling local LFS pointer")
                        .add_blob(&sha256, data.clone())?,
                    StoreType::Shared => cache
                        .as_ref()
                        .expect("no lfs_cache present when handling cache LFS pointer")
                        .add_blob(&sha256, data.clone())?,
                };

                // Unwrap is safe because the only place sha256 could come from is
                // `pending` and all of its entries were put in `key_map`.
                for (key, ptr) in key_map.get(&sha256).unwrap().iter() {
                    let mut file = StoreFile::default();
                    file.content = Some(LazyFile::Lfs(data.clone(), ptr.clone()));

                    // It's important to do this after the self.pointer_origin.get() above, since
                    // found_attributes removes the key from pointer_origin.
                    self.found_attributes(key.clone(), file, Some(StoreType::Shared));
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

    fn found_contentstore(&mut self, key: Key, bytes: Vec<u8>, meta: Metadata) {
        if meta.is_lfs() {
            self.metrics.contentstore.hit_lfsptr(1);
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
            self.metrics.contentstore.hit(1);
            self.found_attributes(key, LazyFile::ContentStore(bytes.into(), meta).into(), None)
        }
    }

    fn fetch_contentstore_inner(
        &mut self,
        store: &ContentStore,
        pending: &mut Vec<StoreKey>,
    ) -> Result<()> {
        let fetch_start = std::time::Instant::now();

        debug!(
            "ContentStore Fallback  - Count = {count}",
            count = pending.len()
        );
        let mut found = 0;
        let mut errors = 0;
        let mut error: Option<String> = None;

        store.prefetch(pending)?;

        for store_key in pending.drain(..) {
            let key = store_key.clone().maybe_into_key().expect(
                "no Key present in StoreKey, even though this should be guaranteed by pending_storekey",
            );
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
                    found += 1;
                    self.found_contentstore(key, blob, meta)
                }
                Err(err) => {
                    self.metrics.contentstore.err(1);
                    errors += 1;
                    if error.is_none() {
                        error.replace(format!("{}: {}", key, err));
                    }
                    self.errors.keyed_error(key, err)
                }
                _ => {
                    self.metrics.contentstore.miss(1);
                }
            }
        }

        self.metrics
            .contentstore
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

        Ok(())
    }

    pub(crate) fn fetch_contentstore(&mut self, store: &ContentStore) {
        let mut pending = self.pending_storekey(FileAttributes::CONTENT);
        if pending.is_empty() {
            return;
        }
        self.metrics.contentstore.fetch(pending.len());
        if let Err(err) = self.fetch_contentstore_inner(store, &mut pending) {
            debug!("ContentStore upper error - Error = {err:?}", err = err);
            self.errors.other_error(err);
            self.metrics.contentstore.err(pending.len());
        }
    }

    // TODO(meyer): Improve how local caching works. At the very least do this in the background.
    // TODO(meyer): Log errors here instead of just ignoring.
    pub(crate) fn derive_computable(
        &mut self,
        aux_cache: Option<&AuxStore>,
        aux_local: Option<&AuxStore>,
    ) {
        if !self.compute_aux_data {
            return;
        }

        for key in self.common.pending.iter().cloned().collect::<Vec<_>>() {
            if let Some(value) = self.common.found.get_mut(&key) {
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

                            match self.key_origin.get(&key).unwrap_or(&StoreType::Shared) {
                                StoreType::Shared => {
                                    if let Some(aux_cache) = aux_cache {
                                        if let Some(aux_data) = new.aux_data {
                                            let _ = aux_cache.put(key.hgid, &aux_data.into());
                                        }
                                    }
                                }
                                StoreType::Local => {
                                    if let Some(aux_local) = aux_local {
                                        if let Some(aux_data) = new.aux_data {
                                            let _ = aux_local.put(key.hgid, &aux_data.into());
                                        }
                                    }
                                }
                            }

                            // TODO(meyer): Extract out a "FetchPending" object like FetchErrors, or otherwise make it possible
                            // to share a "mark complete" implementation while holding a mutable reference to self.found.
                            self.common.pending.remove(&key);
                            self.common.found.remove(&key);
                            let new = new.mask(self.common.request_attrs);
                            let _ = self.common.found_tx.send(Ok((key.clone(), new)));
                            if let Some((ptr, _)) = self.lfs_pointers.remove(&key) {
                                self.pointer_origin.remove(&ptr.sha256());
                            }
                        } else {
                            *value = new;
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn finish(self) {
        self.common.results(self.errors);
    }
}
