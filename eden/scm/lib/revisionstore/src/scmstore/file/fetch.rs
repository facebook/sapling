/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::{hash_map, HashMap, HashSet};
use std::convert::TryInto;
use std::sync::Arc;

use anyhow::{anyhow, Error, Result};
use parking_lot::RwLock;
use tracing::{field, instrument};

use edenapi_types::{FileEntry, FileSpec};

use types::{Key, Sha256};

use crate::{
    datastore::{HgIdDataStore, RemoteDataStore},
    fetch_logger::FetchLogger,
    indexedlogauxstore::{AuxStore, Entry as AuxDataEntry},
    indexedlogdatastore::{Entry, IndexedLogHgIdDataStore},
    indexedlogutil::StoreType,
    lfs::{LfsPointersEntry, LfsRemoteInner, LfsStore, LfsStoreEntry},
    memcache::McData,
    scmstore::{
        attrs::StoreAttrs,
        fetch::{FetchErrors, FetchResults},
        file::{metrics::FileStoreFetchMetrics, LazyFile},
        value::StoreValue,
        FileAttributes, FileAuxData, FileStore, StoreFile,
    },
    util, ContentHash, ContentStore, EdenApiFileStore, ExtStoredPolicy, MemcacheStore, Metadata,
    StoreKey,
};

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

    /// Track fetch metrics,
    metrics: FileStoreFetchMetrics,

    // Config
    extstored_policy: ExtStoredPolicy,
    compute_aux_data: bool,
}

impl FetchState {
    pub(crate) fn new(
        keys: impl Iterator<Item = Key>,
        attrs: FileAttributes,
        file_store: &FileStore,
    ) -> Self {
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
    pub(crate) fn fetch_indexedlog(&mut self, store: &IndexedLogHgIdDataStore, typ: StoreType) {
        let pending = self.pending_nonlfs(FileAttributes::CONTENT);
        if pending.is_empty() {
            return;
        }
        self.metrics.indexedlog.store(typ).fetch(pending.len());
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
    pub(crate) fn fetch_aux_indexedlog(&mut self, store: &AuxStore, typ: StoreType) {
        let pending = self.pending_all(FileAttributes::AUX);
        if pending.is_empty() {
            return;
        }
        self.metrics.aux.store(typ).fetch(pending.len());

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
    pub(crate) fn fetch_lfs(&mut self, store: &LfsStore, typ: StoreType) {
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
    pub(crate) fn fetch_memcache(&mut self, store: &MemcacheStore) {
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

    pub(crate) fn fetch_edenapi(&mut self, store: &EdenApiFileStore) {
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
    pub(crate) fn fetch_lfs_remote(
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
        store.prefetch(&pending)?;
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
                Ok((Some(blob), Some(meta))) => self.found_contentstore(key, blob, meta),
                Err(err) => {
                    self.metrics.contentstore.err(1);
                    self.errors.keyed_error(key, err)
                }
                _ => {
                    self.metrics.contentstore.miss(1);
                }
            }
        }

        Ok(())
    }

    #[instrument(skip(self, store))]
    pub(crate) fn fetch_contentstore(&mut self, store: &ContentStore) {
        let mut pending = self.pending_storekey(FileAttributes::CONTENT);
        if pending.is_empty() {
            return;
        }
        self.metrics.contentstore.fetch(pending.len());
        if let Err(err) = self.fetch_contentstore_inner(store, &mut pending) {
            self.errors.other_error(err);
            self.metrics.contentstore.err(pending.len());
        }
    }

    #[instrument(skip(self))]
    pub(crate) fn derive_computable(&mut self) {
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
    pub(crate) fn write_to_cache(
        &mut self,
        indexedlog_cache: Option<&IndexedLogHgIdDataStore>,
        memcache: Option<&MemcacheStore>,
        aux_cache: Option<&AuxStore>,
        aux_local: Option<&AuxStore>,
    ) {
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
                        if let Some(ref indexedlog_cache) = indexedlog_cache {
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
                        if let Some(ref indexedlog_cache) = indexedlog_cache {
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

                if let Some(ref aux_cache) = aux_cache {
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
                        if let Some(ref aux_cache) = aux_cache {
                            let _ = aux_cache.put(key.hgid, &entry);
                        }
                    }
                    StoreType::Local => {
                        if let Some(ref aux_local) = aux_local {
                            let _ = aux_local.put(key.hgid, &entry);
                        }
                    }
                }
            }
        }
    }

    #[instrument(skip(self))]
    pub(crate) fn finish(mut self) -> FetchResults<StoreFile, FileStoreFetchMetrics> {
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

        FetchResults {
            complete: self.found,
            incomplete,
            other_errors: self.errors.other_errors,
            metrics: self.metrics,
        }
    }
}
