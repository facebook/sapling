/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use ::types::Key;
use ::types::Node;
use ::types::RepoPath;
use ::types::RepoPathBuf;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use crossbeam::channel::unbounded;
use edenapi_types::TreeChildEntry;
use minibytes::Bytes;
use tracing::field;

pub mod types;

use crate::datastore::HgIdDataStore;
use crate::datastore::RemoteDataStore;
use crate::indexedlogdatastore::Entry;
use crate::indexedlogdatastore::IndexedLogHgIdDataStore;
use crate::memcache::MEMCACHE_DELAY;
use crate::scmstore::fetch::CommonFetchState;
use crate::scmstore::fetch::FetchErrors;
use crate::scmstore::fetch::FetchMode;
use crate::scmstore::fetch::FetchResults;
use crate::scmstore::fetch::KeyFetchError;
use crate::scmstore::file::FileStore;
use crate::scmstore::tree::types::LazyTree;
use crate::scmstore::tree::types::StoreTree;
use crate::scmstore::tree::types::TreeAttributes;
use crate::util;
use crate::ContentDataStore;
use crate::ContentMetadata;
use crate::ContentStore;
use crate::Delta;
use crate::EdenApiTreeStore;
use crate::HgIdMutableDeltaStore;
use crate::LegacyStore;
use crate::LocalStore;
use crate::MemcacheStore;
use crate::Metadata;
use crate::RepackLocation;
use crate::StoreKey;
use crate::StoreResult;

pub struct TreeStore {
    /// The "local" indexedlog store. Stores content that is created locally.
    pub indexedlog_local: Option<Arc<IndexedLogHgIdDataStore>>,

    /// The "cache" indexedlog store (previously called "shared"). Stores content downloaded from
    /// a remote store.
    pub indexedlog_cache: Option<Arc<IndexedLogHgIdDataStore>>,

    /// If cache_to_local_cache is true, data found by falling back to a remote store
    /// will the written to indexedlog_cache.
    pub cache_to_local_cache: bool,

    /// If provided, memcache will be checked before other remote stores
    pub memcache: Option<Arc<MemcacheStore>>,

    /// If cache_to_memcache is true, data found by falling back to another remote store
    // will be written to memcache.
    pub cache_to_memcache: bool,

    /// An EdenApi Client, EdenApiTreeStore provides the tree-specific subset of EdenApi functionality
    /// used by TreeStore.
    pub edenapi: Option<Arc<EdenApiTreeStore>>,

    /// Hook into the legacy storage architecture, if we fall back to this and succeed, we
    /// should alert / log something, as this should never happen if TreeStore is implemented
    /// correctly.
    pub contentstore: Option<Arc<ContentStore>>,

    /// A FileStore, which can be used for fetching and caching file aux data for a tree.
    pub filestore: Option<Arc<FileStore>>,

    pub creation_time: Instant,

    pub flush_on_drop: bool,
}

impl Drop for TreeStore {
    fn drop(&mut self) {
        if self.flush_on_drop {
            let _ = self.flush();
        }
    }
}

impl TreeStore {
    pub fn fetch_batch(
        &self,
        reqs: impl Iterator<Item = Key>,
        fetch_mode: FetchMode,
    ) -> FetchResults<StoreTree> {
        let (found_tx, found_rx) = unbounded();
        let found_tx2 = found_tx.clone();
        let mut common: CommonFetchState<StoreTree> =
            CommonFetchState::new(reqs, TreeAttributes::CONTENT, found_tx);

        let keys_len = common.pending_len();

        let indexedlog_cache = self.indexedlog_cache.clone();
        let indexedlog_local = self.indexedlog_local.clone();
        let memcache = self.memcache.clone();
        let edenapi = self.edenapi.clone();

        let contentstore = self.contentstore.clone();
        let creation_time = self.creation_time;
        let cache_to_memcache = self.cache_to_memcache;
        let cache_to_local_cache = self.cache_to_local_cache;
        let (aux_local, aux_cache) = if let Some(ref filestore) = self.filestore {
            (filestore.aux_local.clone(), filestore.aux_cache.clone())
        } else {
            (None, None)
        };
        let process_func = move || -> Result<()> {
            if let Some(ref indexedlog_cache) = indexedlog_cache {
                let pending: Vec<_> = common
                    .pending(TreeAttributes::CONTENT, false)
                    .map(|(key, _attrs)| key.clone())
                    .collect();
                for key in pending.into_iter() {
                    if let Some(entry) = indexedlog_cache.get_entry(key)? {
                        tracing::trace!("{:?} found in cache", &entry.key());
                        common.found(entry.key().clone(), LazyTree::IndexedLog(entry).into());
                    }
                }
            }

            if let Some(ref indexedlog_local) = indexedlog_local {
                let pending: Vec<_> = common
                    .pending(TreeAttributes::CONTENT, false)
                    .map(|(key, _attrs)| key.clone())
                    .collect();
                for key in pending.into_iter() {
                    if let Some(entry) = indexedlog_local.get_entry(key)? {
                        tracing::trace!("{:?} found in local", &entry.key());
                        common.found(entry.key().clone(), LazyTree::IndexedLog(entry).into());
                    }
                }
            }

            if let FetchMode::AllowRemote = fetch_mode {
                if use_memcache(creation_time) {
                    if let Some(ref memcache) = memcache {
                        let pending: Vec<_> = common
                            .pending(TreeAttributes::CONTENT, false)
                            .map(|(key, _attrs)| key.clone())
                            .collect();

                        if !pending.is_empty() {
                            for entry in memcache.get_data_iter(&pending)? {
                                let entry = entry?;
                                let key = entry.key.clone();
                                let entry = LazyTree::Memcache(entry);
                                if indexedlog_cache.is_some() && cache_to_local_cache {
                                    if let Some(entry) =
                                        entry.indexedlog_cache_entry(key.clone())?
                                    {
                                        indexedlog_cache.as_ref().unwrap().put_entry(entry)?;
                                    }
                                }
                                tracing::trace!("{:?} found in memcache", &key);
                                common.found(key, entry.into());
                            }
                        }
                    }
                }
            }

            if let FetchMode::AllowRemote = fetch_mode {
                if let Some(ref edenapi) = edenapi {
                    let pending: Vec<_> = common
                        .pending(TreeAttributes::CONTENT, false)
                        .map(|(key, _attrs)| key.clone())
                        .collect();
                    if !pending.is_empty() {
                        let span = tracing::info_span!(
                            "fetch_edenapi",
                            downloaded = field::Empty,
                            uploaded = field::Empty,
                            requests = field::Empty,
                            time = field::Empty,
                            latency = field::Empty,
                            download_speed = field::Empty,
                        );
                        let _enter = span.enter();
                        tracing::debug!(
                            "attempt to fetch {} keys from edenapi ({:?})",
                            pending.len(),
                            edenapi.url()
                        );
                        let attributes = if aux_local.is_some() {
                            Some(edenapi_types::TreeAttributes {
                                child_metadata: true,
                                ..edenapi_types::TreeAttributes::default()
                            })
                        } else {
                            None
                        };
                        let response = edenapi
                            .trees_blocking(pending, attributes)
                            .map_err(|e| e.tag_network())?;
                        for entry in response.entries {
                            let entry = entry?;
                            let key = entry.key.clone();
                            if let Some(ref aux_local) = aux_local {
                                if let Some(ref children) = entry.children {
                                    for file_entry in children {
                                        let file_entry = match file_entry {
                                            Ok(file_entry) => file_entry,
                                            Err(err) => {
                                                // not failing tree fetching for aux related problems
                                                tracing::warn!(
                                                    "Error fetching child entry: {:?}",
                                                    err
                                                );
                                                continue;
                                            }
                                        };
                                        if let TreeChildEntry::File(file_entry) = file_entry {
                                            if let Some(metadata) = file_entry.file_metadata {
                                                let aux_entry = crate::indexedlogauxstore::Entry {
                                                    total_size: metadata.size.unwrap(),
                                                    content_id: metadata.content_id.unwrap(),
                                                    content_sha1: metadata.content_sha1.unwrap(),
                                                    content_sha256: metadata
                                                        .content_sha256
                                                        .unwrap(),
                                                };
                                                if let Some(ref aux_cache) = aux_cache {
                                                    aux_cache
                                                        .put(file_entry.key.hgid, &aux_entry)?;
                                                } else {
                                                    aux_local
                                                        .put(file_entry.key.hgid, &aux_entry)?;
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    // this is odd, need to log
                                    tracing::warn!(
                                        "No children returned when requested tree {}",
                                        entry.key.hgid
                                    );
                                }
                            }
                            let entry = LazyTree::EdenApi(entry);
                            if indexedlog_cache.is_some() && cache_to_local_cache {
                                if let Some(entry) = entry.indexedlog_cache_entry(key.clone())? {
                                    indexedlog_cache.as_ref().unwrap().put_entry(entry)?;
                                }
                            }
                            if memcache.is_some()
                                && cache_to_memcache
                                && use_memcache(creation_time)
                            {
                                if let Some(entry) = entry.indexedlog_cache_entry(key.clone())? {
                                    memcache.as_ref().unwrap().add_mcdata(entry.try_into()?);
                                }
                            }
                            common.found(key, entry.into());
                        }
                        util::record_edenapi_stats(&span, &response.stats);
                    }
                } else {
                    tracing::debug!("no EdenApi associated with TreeStore");
                }
            }

            if let FetchMode::AllowRemote = fetch_mode {
                if let Some(ref contentstore) = contentstore {
                    let pending: Vec<_> = common
                        .pending(TreeAttributes::CONTENT, false)
                        .map(|(key, _attrs)| StoreKey::HgId(key.clone()))
                        .collect();
                    if !pending.is_empty() {
                        tracing::debug!(
                            "attempt to fetch {} keys from contentstore",
                            pending.len()
                        );
                        contentstore.prefetch(&pending)?;

                        let pending = pending.into_iter().map(|key| match key {
                            StoreKey::HgId(key) => key,
                            // Safe because we constructed pending with only StoreKey::HgId above
                            // we're just re-using the already allocated paths in the keys
                            _ => unreachable!("unexpected non-HgId StoreKey"),
                        });

                        for key in pending {
                            let store_key = StoreKey::HgId(key.clone());
                            let blob = match contentstore.get(store_key.clone())? {
                                StoreResult::Found(v) => Some(v),
                                StoreResult::NotFound(_k) => None,
                            };
                            let meta = match contentstore.get_meta(store_key)? {
                                StoreResult::Found(v) => Some(v),
                                StoreResult::NotFound(_k) => None,
                            };

                            if let (Some(blob), Some(meta)) = (blob, meta) {
                                // We don't write to local indexedlog or memcache for contentstore fallbacks because
                                // contentstore handles that internally.
                                tracing::trace!("{:?} found in contentstore", &key);
                                common.found(key, LazyTree::ContentStore(blob.into(), meta).into());
                            }
                        }
                    }
                }
            }

            // TODO(meyer): Report incomplete / not found, handle errors better instead of just always failing the batch, etc
            common.results(FetchErrors::new());
            Ok(())
        };
        let process_func_errors = move || {
            if let Err(err) = process_func() {
                let _ = found_tx2.send(Err(KeyFetchError::Other(err)));
            }
        };

        // Only kick off a thread if there's a substantial amount of work.
        if keys_len > 1000 {
            std::thread::spawn(process_func_errors);
        } else {
            process_func_errors();
        }

        FetchResults::new(Box::new(found_rx.into_iter()))
    }

    fn write_batch(&self, entries: impl Iterator<Item = (Key, Bytes, Metadata)>) -> Result<()> {
        if let Some(ref indexedlog_local) = self.indexedlog_local {
            for (key, bytes, meta) in entries {
                indexedlog_local.put_entry(Entry::new(key, bytes, meta))?;
            }
        }
        Ok(())
    }

    pub fn empty() -> Self {
        TreeStore {
            indexedlog_local: None,

            indexedlog_cache: None,
            cache_to_local_cache: true,

            memcache: None,
            cache_to_memcache: true,

            edenapi: None,

            contentstore: None,

            filestore: None,
            creation_time: Instant::now(),
            flush_on_drop: true,
        }
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

        result
    }

    pub fn refresh(&self) -> Result<()> {
        if let Some(contentstore) = self.contentstore.as_ref() {
            contentstore.refresh()?;
        }
        self.flush()
    }
}

fn use_memcache(creation_time: Instant) -> bool {
    // Only use memcache if the process has been around a while. It takes 2s to setup, which
    // hurts responiveness for short commands.
    creation_time.elapsed() > MEMCACHE_DELAY
}

impl LegacyStore for TreeStore {
    /// Returns only the local cache / shared stores, in place of the local-only stores, such that writes will go directly to the local cache.
    /// For compatibility with ContentStore::get_shared_mutable
    fn get_shared_mutable(&self) -> Arc<dyn HgIdMutableDeltaStore> {
        // this is infallible in ContentStore so panic if there are no shared/cache stores.
        assert!(
            self.indexedlog_cache.is_some(),
            "cannot get shared_mutable, no shared / local cache stores available"
        );
        Arc::new(TreeStore {
            indexedlog_local: self.indexedlog_cache.clone(),
            indexedlog_cache: None,
            cache_to_local_cache: false,

            memcache: None,
            cache_to_memcache: false,

            edenapi: None,
            contentstore: None,

            filestore: None,
            creation_time: Instant::now(),
            flush_on_drop: true,
        })
    }

    fn get_logged_fetches(&self) -> HashSet<RepoPathBuf> {
        unimplemented!(
            "get_logged_fetches is not implemented for trees, it should only ever be falled for files"
        );
    }

    fn get_file_content(&self, _key: &Key) -> Result<Option<Bytes>> {
        unimplemented!(
            "get_file_content is not implemented for trees, it should only ever be falled for files"
        );
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

impl HgIdDataStore for TreeStore {
    // Fetch the raw content of a single TreeManifest blob
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        Ok(
            match self
                .fetch_batch(std::iter::once(key.clone()).filter_map(StoreKey::maybe_into_key), FetchMode::AllowRemote)
                .single()?
            {
                Some(entry) => StoreResult::Found(entry.content.expect("content attribute not found despite being requested and returned as complete").hg_content()?.into_vec()),
                None => StoreResult::NotFound(key),
            },
        )
    }

    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        Ok(
            match self
                .fetch_batch(
                    std::iter::once(key.clone()).filter_map(StoreKey::maybe_into_key),
                    FetchMode::AllowRemote,
                )
                .single()?
            {
                // This is currently in a bit of an awkward state, as revisionstore metadata is no longer used for trees
                // (it should always be default), but the get_meta function should return StoreResult::Found
                // only when the content is available. Thus, we request the tree content, but ignore it and just
                // return default metadata when it's found, and otherwise report StoreResult::NotFound.
                // TODO(meyer): Replace this with an presence check once support for separate fetch and return attrs
                // is added.
                Some(_e) => StoreResult::Found(Metadata::default()),
                None => StoreResult::NotFound(key),
            },
        )
    }

    fn refresh(&self) -> Result<()> {
        self.refresh()
    }
}

impl RemoteDataStore for TreeStore {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        Ok(self
            .fetch_batch(
                keys.iter().cloned().filter_map(StoreKey::maybe_into_key),
                FetchMode::AllowRemote,
            )
            .missing()?
            .into_iter()
            .map(StoreKey::HgId)
            .collect())
    }

    fn upload(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        Ok(keys.to_vec())
    }
}

impl LocalStore for TreeStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        let mut missing: Vec<_> = keys.to_vec();

        missing = if let Some(ref indexedlog_cache) = self.indexedlog_cache {
            missing
                .into_iter()
                .filter(
                    |sk| match sk.maybe_as_key().map(|k| indexedlog_cache.get_raw_entry(k)) {
                        Some(Ok(Some(_))) => false,
                        None | Some(Err(_)) | Some(Ok(None)) => true,
                    },
                )
                .collect()
        } else {
            missing
        };

        missing = if let Some(ref indexedlog_local) = self.indexedlog_local {
            missing
                .into_iter()
                .filter(
                    |sk| match sk.maybe_as_key().map(|k| indexedlog_local.get_raw_entry(k)) {
                        Some(Ok(Some(_))) => false,
                        None | Some(Err(_)) | Some(Ok(None)) => true,
                    },
                )
                .collect()
        } else {
            missing
        };

        Ok(missing)
    }
}

impl HgIdMutableDeltaStore for TreeStore {
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
        Ok(None)
    }
}

// TODO(meyer): Content addressing not supported at all for trees. I could look for HgIds present here and fetch with
// that if available, but I feel like there's probably something wrong if this is called for trees. Do we need to implement
// this at all for trees?
impl ContentDataStore for TreeStore {
    fn blob(&self, key: StoreKey) -> Result<StoreResult<Bytes>> {
        Ok(StoreResult::NotFound(key))
    }

    fn metadata(&self, key: StoreKey) -> Result<StoreResult<ContentMetadata>> {
        Ok(StoreResult::NotFound(key))
    }
}

impl storemodel::TreeStore for TreeStore {
    fn get(&self, path: &RepoPath, node: Node) -> Result<minibytes::Bytes> {
        if node.is_null() {
            return Ok(Default::default());
        }

        let key = Key::new(path.to_owned(), node);
        match self
            .fetch_batch(std::iter::once(key.clone()), FetchMode::AllowRemote)
            .single()?
        {
            Some(entry) => Ok(entry.content.expect("no tree content").hg_content()?),
            None => Err(anyhow!("key {:?} not found in manifest", key)),
        }
    }

    fn prefetch(&self, keys: Vec<Key>) -> Result<()> {
        self.fetch_batch(keys.into_iter(), FetchMode::AllowRemote)
            .consume();
        Ok(())
    }

    fn insert(&self, _path: &RepoPath, _node: Node, _data: Bytes) -> Result<()> {
        unimplemented!("not needed yet");
    }
}

impl storemodel::RefreshableTreeStore for TreeStore {
    fn refresh(&self) -> Result<()> {
        self.refresh()
    }
}
