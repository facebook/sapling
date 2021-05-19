/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// TODO(meyer): Remove this
#![allow(dead_code)]
use std::collections::{HashMap, HashSet};
use std::convert::{TryFrom, TryInto};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, bail, ensure, Error, Result};
use parking_lot::RwLock;

use edenapi_types::FileEntry;
use minibytes::Bytes;
use types::{HgId, Key, Sha256};

use crate::{
    datastore::{strip_metadata, HgIdDataStore, HgIdMutableDeltaStore, RemoteDataStore},
    indexedlogdatastore::{Entry, IndexedLogHgIdDataStore},
    lfs::{
        lfs_from_hg_file_blob, rebuild_metadata, LfsPointersEntry, LfsRemoteInner, LfsStore,
        LfsStoreEntry,
    },
    memcache::McData,
    ContentDataStore, ContentHash, ContentMetadata, ContentStore, Delta, EdenApiFileStore,
    ExtStoredPolicy, LocalStore, MemcacheStore, Metadata, StoreKey, StoreResult,
};

pub struct FileStore {
    // Config
    pub(crate) extstored_policy: ExtStoredPolicy,
    pub(crate) lfs_threshold_bytes: Option<u64>,
    pub(crate) cache_to_local_cache: bool,
    pub(crate) cache_to_memcache: bool,

    // Local-only stores
    pub(crate) indexedlog_local: Option<Arc<IndexedLogHgIdDataStore>>,
    pub(crate) lfs_local: Option<Arc<LfsStore>>,

    // Local non-lfs cache aka shared store
    pub(crate) indexedlog_cache: Option<Arc<IndexedLogHgIdDataStore>>,

    // Local LFS cache aka shared store
    pub(crate) lfs_cache: Option<Arc<LfsStore>>,

    // Mecache
    pub(crate) memcache: Option<Arc<MemcacheStore>>,

    // Remote stores
    pub(crate) lfs_remote: Option<Arc<LfsRemoteInner>>,
    pub(crate) edenapi: Option<Arc<EdenApiFileStore>>,

    // Legacy ContentStore fallback
    pub(crate) contentstore: Option<Arc<ContentStore>>,
}

impl Drop for FileStore {
    /// The shared store is a cache, so let's flush all pending data when the `ContentStore` goes
    /// out of scope.
    fn drop(&mut self) {
        if let Some(ref indexedlog_local) = self.indexedlog_local {
            // TODO(meyer): Drop can't fail, so we ignore errors here. We should probably attempt to log them instead.
            let _ = indexedlog_local.flush_log();
        }
        if let Some(ref indexedlog_cache) = self.indexedlog_cache {
            let _ = indexedlog_cache.flush_log();
        }
        if let Some(ref lfs_local) = self.lfs_local {
            let _ = lfs_local.flush();
        }
        if let Some(ref lfs_cache) = self.lfs_cache {
            let _ = lfs_cache.flush();
        }
    }
}

#[derive(Debug)]
pub struct FileStoreFetch {
    complete: HashMap<Key, LazyFile>,
    incomplete: HashMap<Key, Vec<Error>>,
    other_errors: Vec<Error>,
}

impl FileStore {
    pub fn fetch(&self, keys: impl Iterator<Item = Key>) -> FileStoreFetch {
        let mut state = FetchState::new(keys, self.extstored_policy);

        if let Some(ref indexedlog_cache) = self.indexedlog_cache {
            state.fetch_indexedlog(indexedlog_cache, LocalStoreType::Cache);
        }

        if let Some(ref indexedlog_local) = self.indexedlog_local {
            state.fetch_indexedlog(indexedlog_local, LocalStoreType::Local);
        }

        if let Some(ref lfs_cache) = self.lfs_cache {
            state.fetch_lfs(lfs_cache, LocalStoreType::Cache);
        }

        if let Some(ref lfs_local) = self.lfs_local {
            state.fetch_lfs(lfs_local, LocalStoreType::Local);
        }

        if let Some(ref memcache) = self.memcache {
            state.fetch_memcache(memcache);
        }

        if let Some(ref edenapi) = self.edenapi {
            state.fetch_edenapi(edenapi);
        }

        if let Some(ref lfs_remote) = self.lfs_remote {
            state.fetch_lfs_remote(lfs_remote, self.lfs_local.clone(), self.lfs_cache.clone());
        }

        if let Some(ref contentstore) = self.contentstore {
            state.fetch_contentstore(contentstore);
        }

        state.write_to_cache(
            self.indexedlog_cache.as_ref().and_then(|s| {
                if self.cache_to_local_cache {
                    Some(s.as_ref())
                } else {
                    None
                }
            }),
            self.memcache.as_ref().and_then(|s| {
                if self.cache_to_memcache {
                    Some(s.as_ref())
                } else {
                    None
                }
            }),
        );

        state.finish()
    }

    pub fn write_batch(&self, entries: impl Iterator<Item = (Key, Bytes, Metadata)>) -> Result<()> {
        let mut indexedlog_local = self.indexedlog_local.as_ref().map(|l| l.write_lock());
        for (key, bytes, meta) in entries {
            ensure!(
                !meta.is_lfs(),
                "writing LFS pointers directly via ScmStore is not supported"
            );
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

    pub fn local(&self) -> Self {
        FileStore {
            extstored_policy: self.extstored_policy.clone(),
            lfs_threshold_bytes: self.lfs_threshold_bytes.clone(),

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
        }
    }
}

/// A minimal file enum that simply wraps the possible underlying file types,
/// with no processing (so Entry might have the wrong Key.path, etc.)
#[derive(Debug)]
enum LazyFile {
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

    /// The file content, as would be found in the working copy (stripped of copy header)
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

    fn metadata(&self) -> Result<Metadata> {
        use LazyFile::*;
        Ok(match self {
            IndexedLog(ref entry) => entry.metadata().clone(),
            Lfs(_, ref ptr) => Metadata {
                size: Some(ptr.size()),
                flags: None,
            },
            ContentStore(_, ref meta) => meta.clone(),
            EdenApi(ref entry) => entry.metadata().clone(),
            Memcache(ref entry) => entry.metadata.clone(),
        })
    }

    /// Convert the LazyFile to an indexedlog Entry, if it should ever be written to IndexedLog cache
    fn indexedlog_cache_entry(&self, key: Key) -> Result<Option<Entry>> {
        use LazyFile::*;
        Ok(match self {
            IndexedLog(ref entry) => Some(entry.clone().with_key(key)),
            EdenApi(ref entry) => Some(Entry::new(
                key,
                entry.data()?.into(),
                entry.metadata().clone(),
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
        if e.metadata().is_lfs() {
            Ok(LfsPointersEntry::from_bytes(e.data()?, e.key().hgid)?)
        } else {
            bail!("failed to convert EdenApi FileEntry to LFS pointer, but is_lfs is false")
        }
    }
}

#[derive(Copy, Clone, Debug)]
enum LocalStoreType {
    Local,
    Cache,
}

pub struct FetchState {
    // The originally requested keys that haven't been found yet.
    incomplete_hgid: HashSet<Key>,

    // Requests that we've found an LFS pointer for but no content so far
    incomplete_pointers: HashMap<Key, LfsPointersEntry>,
    // Metadata about LFS pointers we've found (originating store of the LFS pointer)
    pointer_meta: Arc<RwLock<HashMap<Sha256, LocalStoreType>>>,

    // Completed requests accumulated here
    complete: HashMap<Key, LazyFile>,

    // Store the last error seen for a key
    fetch_errors: HashMap<Key, Vec<Error>>,
    // Store errors that don't just apply to a single key
    other_errors: Vec<Error>,

    // Requests that required remote fallback, might be cached locally
    found_in_memcache: HashSet<Key>,
    found_in_edenapi: HashSet<Key>,

    // Config
    extstored_policy: ExtStoredPolicy,
}

impl FetchState {
    fn new(keys: impl Iterator<Item = Key>, extstored_policy: ExtStoredPolicy) -> Self {
        FetchState {
            incomplete_hgid: keys.collect(),

            incomplete_pointers: HashMap::new(),
            pointer_meta: Arc::new(RwLock::new(HashMap::new())),

            complete: HashMap::new(),

            fetch_errors: HashMap::new(),
            other_errors: vec![],

            found_in_memcache: HashSet::new(),
            found_in_edenapi: HashSet::new(),

            extstored_policy,
        }
    }

    /// Returns all incomplete HgId requests, not including those that we've discovered correspond to an LFS pointer.
    fn pending_hgid(&self) -> Vec<Key> {
        self.incomplete_hgid.iter().cloned().collect()
    }

    /// Returns all incomplete requests as StoreKey, returning discovered LFS pointers as StoreKey::Content.
    /// Every StoreKey returned here is guaranteed to have a Key available, so unwrapping is safe.
    fn pending_all(&self) -> Vec<StoreKey> {
        self.incomplete_hgid
            .iter()
            .cloned()
            .map(StoreKey::HgId)
            .chain(
                self.incomplete_pointers.iter().map(|(k, v)| {
                    StoreKey::Content(ContentHash::Sha256(v.sha256()), Some(k.clone()))
                }),
            )
            .collect()
    }

    fn mark_complete(&mut self, key: &Key) {
        self.incomplete_hgid.remove(key);
        self.incomplete_pointers.remove(key);
    }

    fn found_error(&mut self, maybe_key: Option<Key>, err: Error) {
        if let Some(key) = maybe_key {
            self.fetch_errors
                .entry(key)
                .or_insert_with(Vec::new)
                .push(err);
        } else {
            self.other_errors.push(err);
        }
    }

    fn found_pointer(&mut self, key: Key, ptr: LfsPointersEntry, typ: LocalStoreType) {
        self.mark_complete(&key);
        let sha256 = ptr.sha256();
        // Overwrite LocalStoreType::Local with LocalStoreType::Cache, but not vice versa
        match typ {
            LocalStoreType::Cache => {
                self.pointer_meta.write().insert(sha256, typ);
            }
            LocalStoreType::Local => {
                self.pointer_meta.write().entry(sha256).or_insert(typ);
            }
        }
        self.incomplete_pointers.insert(key, ptr);
    }

    fn found_file(&mut self, key: Key, f: LazyFile) {
        self.mark_complete(&key);
        self.complete.insert(key, f);
    }

    fn found_indexedlog(&mut self, key: Key, entry: Entry, typ: LocalStoreType) {
        if entry.metadata().is_lfs() {
            if self.extstored_policy == ExtStoredPolicy::Use {
                match entry.try_into() {
                    Ok(ptr) => self.found_pointer(key, ptr, typ),
                    Err(err) => self.found_error(Some(key), err),
                }
            }
        } else {
            self.found_file(key, LazyFile::IndexedLog(entry))
        }
    }

    fn fetch_indexedlog(&mut self, store: &IndexedLogHgIdDataStore, typ: LocalStoreType) {
        let pending = self.pending_hgid();
        let store = store.read_lock();
        for key in pending.into_iter() {
            let res = store.get_raw_entry(&key);
            match res {
                Ok(Some(entry)) => self.found_indexedlog(key, entry, typ),
                Ok(None) => {}
                Err(err) => self.found_error(Some(key), err),
            }
        }
    }

    fn found_lfs(&mut self, key: Key, entry: LfsStoreEntry, typ: LocalStoreType) {
        match entry {
            LfsStoreEntry::PointerAndBlob(ptr, blob) => {
                self.found_file(key, LazyFile::Lfs(blob, ptr))
            }
            LfsStoreEntry::PointerOnly(ptr) => self.found_pointer(key, ptr, typ),
        }
    }

    fn fetch_lfs(&mut self, store: &LfsStore, typ: LocalStoreType) {
        let pending = self.pending_all();
        for store_key in pending.into_iter() {
            let key = store_key.clone().maybe_into_key().expect(
                "no Key present in StoreKey, even though this should be guaranteed by pending_all",
            );
            match store.fetch_available(&store_key) {
                Ok(Some(entry)) => self.found_lfs(key, entry, typ),
                Ok(None) => {}
                Err(err) => self.found_error(Some(key), err),
            }
        }
    }

    fn found_memcache(&mut self, entry: McData) {
        let key = entry.key.clone();
        if entry.metadata.is_lfs() {
            match entry.try_into() {
                Ok(ptr) => self.found_pointer(key, ptr, LocalStoreType::Cache),
                Err(err) => self.found_error(Some(key), err),
            }
        } else {
            self.found_in_memcache.insert(key.clone());
            self.found_file(key, LazyFile::Memcache(entry));
        }
    }

    fn fetch_memcache_inner(&mut self, store: &MemcacheStore) -> Result<()> {
        let pending = self.pending_hgid();
        for res in store.get_data_iter(&pending)?.into_iter() {
            match res {
                Ok(mcdata) => self.found_memcache(mcdata),
                Err(err) => self.found_error(None, err),
            }
        }
        Ok(())
    }

    fn fetch_memcache(&mut self, store: &MemcacheStore) {
        if let Err(err) = self.fetch_memcache_inner(store) {
            self.found_error(None, err);
        }
    }

    fn found_edenapi(&mut self, entry: FileEntry) {
        let key = entry.key.clone();
        if entry.metadata().is_lfs() {
            match entry.try_into() {
                Ok(ptr) => self.found_pointer(key, ptr, LocalStoreType::Cache),
                Err(err) => self.found_error(Some(key), err),
            }
        } else {
            self.found_in_edenapi.insert(key.clone());
            self.found_file(key, LazyFile::EdenApi(entry));
        }
    }

    fn fetch_edenapi_inner(&mut self, store: &EdenApiFileStore) -> Result<()> {
        let pending = self.pending_hgid();
        for entry in store.files_blocking(pending, None)?.entries.into_iter() {
            self.found_edenapi(entry);
        }
        Ok(())
    }

    fn fetch_edenapi(&mut self, store: &EdenApiFileStore) {
        if let Err(err) = self.fetch_edenapi_inner(store) {
            self.found_error(None, err);
        }
    }

    fn fetch_lfs_remote_inner(
        &mut self,
        store: &LfsRemoteInner,
        local: Option<Arc<LfsStore>>,
        cache: Option<Arc<LfsStore>>,
    ) -> Result<()> {
        let pending: HashSet<_> = self
            .incomplete_pointers
            .iter()
            .map(|(_k, v)| (v.sha256(), v.size() as usize))
            .collect();
        // Fetch & write to local LFS stores
        store.batch_fetch(&pending, {
            let lfs_local = local.clone();
            let lfs_cache = cache.clone();
            let pointer_meta = self.pointer_meta.clone();
            move |sha256, data| -> Result<()> {
                match pointer_meta.read().get(&sha256).ok_or_else(|| {
                    anyhow!(
                        "no source found for Sha256; received unexpected Sha256 from LFS server"
                    )
                })? {
                    LocalStoreType::Local => lfs_local
                        .as_ref()
                        .expect("no lfs_local present when handling local LFS pointer")
                        .add_blob(&sha256, data),
                    LocalStoreType::Cache => lfs_cache
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
            self.fetch_lfs(lfs_cache, LocalStoreType::Cache)
        }

        if let Some(ref lfs_local) = local {
            self.fetch_lfs(lfs_local, LocalStoreType::Local)
        }

        Ok(())
    }

    fn fetch_lfs_remote(
        &mut self,
        store: &LfsRemoteInner,
        local: Option<Arc<LfsStore>>,
        cache: Option<Arc<LfsStore>>,
    ) {
        if let Err(err) = self.fetch_lfs_remote_inner(store, local, cache) {
            self.found_error(None, err);
        }
    }

    fn found_contentstore(&mut self, key: Key, bytes: Vec<u8>, meta: Metadata) {
        if meta.is_lfs() {
            // Do nothing. We're trying to avoid exposing LFS pointers to the consumer of this API, and
            // if we're here, both we and ContentStore have already tried querying the remotes.
            // We very well may need to expose LFS Pointers to the caller in the end (to match ContentStore's
            // ExtStoredPolicy behavior) in which case we'll do something here.
        } else {
            self.found_file(key, LazyFile::ContentStore(bytes.into(), meta))
        }
    }

    fn fetch_contentstore_inner(&mut self, store: &ContentStore) -> Result<()> {
        let pending = self.pending_all();
        store.prefetch(&pending)?;
        for store_key in pending.into_iter() {
            let key = store_key.clone().maybe_into_key().expect(
                "no Key present in StoreKey, even though this should be guaranteed by pending_all",
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
                Err(err) => self.found_error(Some(key), err),
                _ => {}
            }
        }

        Ok(())
    }

    fn fetch_contentstore(&mut self, store: &ContentStore) {
        if let Err(err) = self.fetch_contentstore_inner(store) {
            self.found_error(None, err);
        }
    }

    // TODO(meyer): Improve how local caching works. At the very least do this in the background.
    // TODO(meyer): Log errors here instead of just ignoring.
    fn write_to_cache(
        &mut self,
        indexedlog_cache: Option<&IndexedLogHgIdDataStore>,
        memcache: Option<&MemcacheStore>,
    ) {
        let mut indexedlog_cache = indexedlog_cache.map(|s| s.write_lock());

        for key in self.found_in_edenapi.drain() {
            if let Ok(Some(cache_entry)) = self.complete[&key].indexedlog_cache_entry(key) {
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

        for key in self.found_in_memcache.drain() {
            if let Ok(Some(cache_entry)) = self.complete[&key].indexedlog_cache_entry(key) {
                if let Some(ref mut indexedlog_cache) = indexedlog_cache {
                    let _ = indexedlog_cache.put_entry(cache_entry);
                }
            }
        }
    }
    fn finish(self) -> FileStoreFetch {
        // Collect all incomplete request Keys
        let incomplete_hgid = self.incomplete_hgid.into_iter();
        let incomplete_pointers = self.incomplete_pointers.into_iter().map(|(key, _ptr)| key);

        // Combine and collect errors
        let mut incomplete = self.fetch_errors;
        for key in incomplete_hgid.chain(incomplete_pointers) {
            incomplete.entry(key).or_insert_with(Vec::new);
        }

        FileStoreFetch {
            complete: self.complete,
            incomplete,
            other_errors: self.other_errors,
        }
    }
}

impl HgIdDataStore for FileStore {
    // Fetch the raw content of a single TreeManifest blob
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        Ok(
            match self
                .fetch(std::iter::once(key.clone()).filter_map(|sk| sk.maybe_into_key()))
                .complete
                .drain()
                .next()
            {
                Some((_, mut entry)) => StoreResult::Found(entry.hg_content()?.into_vec()),
                None => StoreResult::NotFound(key),
            },
        )
    }

    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        Ok(
            match self
                .fetch(std::iter::once(key.clone()).filter_map(|sk| sk.maybe_into_key()))
                .complete
                .drain()
                .next()
            {
                Some((_, entry)) => StoreResult::Found(entry.metadata()?),
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
            .fetch(keys.iter().cloned().filter_map(|sk| sk.maybe_into_key()))
            .incomplete
            .into_iter()
            .map(|(k, _)| k)
            .map(StoreKey::HgId)
            .collect())
    }

    fn upload(&self, _keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        unimplemented!()
        //Ok(keys.to_vec())
    }
}

impl LocalStore for FileStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        Ok(self
            .local()
            .fetch(keys.iter().cloned().filter_map(|sk| sk.maybe_into_key()))
            .incomplete
            .into_iter()
            .map(|(k, _)| k)
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
        if let Some(ref indexedlog_local) = self.indexedlog_local {
            indexedlog_local.flush_log()?;
        }
        if let Some(ref indexedlog_cache) = self.indexedlog_cache {
            indexedlog_cache.flush_log()?;
        }
        if let Some(ref lfs_local) = self.lfs_local {
            lfs_local.flush()?;
        }
        if let Some(ref lfs_cache) = self.lfs_cache {
            lfs_cache.flush()?;
        }
        Ok(None)
    }
}

// TODO(meyer): Content addressing not supported at all for trees. I could look for HgIds present here and fetch with
// that if available, but I feel like there's probably something wrong if this is called for trees.
impl ContentDataStore for FileStore {
    fn blob(&self, key: StoreKey) -> Result<StoreResult<Bytes>> {
        Ok(
            match self
                .fetch(std::iter::once(key.clone()).filter_map(|sk| sk.maybe_into_key()))
                .complete
                .drain()
                .next()
            {
                Some((_sk, mut entry)) => StoreResult::Found(entry.file_content()?),
                None => StoreResult::NotFound(key),
            },
        )
    }

    fn metadata(&self, key: StoreKey) -> Result<StoreResult<ContentMetadata>> {
        Ok(
            match self
                .fetch(std::iter::once(key.clone()).filter_map(|sk| sk.maybe_into_key()))
                .complete
                .drain()
                .next()
            {
                Some((_sk, LazyFile::Lfs(_blob, pointer))) => StoreResult::Found(pointer.into()),
                Some(_) => StoreResult::NotFound(key),
                None => StoreResult::NotFound(key),
            },
        )
    }
}
