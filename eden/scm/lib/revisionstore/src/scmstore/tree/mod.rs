/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// TODO(meyer): Remove this
#![allow(dead_code)]
use std::collections::HashSet;
use std::convert::TryInto;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{bail, Result};

use ::types::{Key, RepoPathBuf};
use minibytes::Bytes;
use tracing::field;

pub mod types;

use crate::{
    datastore::{HgIdDataStore, RemoteDataStore},
    indexedlogdatastore::{Entry, IndexedLogHgIdDataStore},
    memcache::MEMCACHE_DELAY,
    scmstore::{
        fetch::{CommonFetchState, FetchErrors, FetchResults},
        tree::types::{LazyTree, StoreTree, TreeAttributes},
    },
    util, ContentDataStore, ContentMetadata, ContentStore, Delta, EdenApiTreeStore,
    HgIdMutableDeltaStore, LegacyStore, LocalStore, MemcacheStore, Metadata, RepackLocation,
    StoreKey, StoreResult,
};

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

    pub creation_time: Instant,
}

impl Drop for TreeStore {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

impl TreeStore {
    pub fn fetch_batch(
        &self,
        reqs: impl Iterator<Item = Key>,
    ) -> Result<FetchResults<StoreTree, ()>> {
        let mut common: CommonFetchState<StoreTree> =
            CommonFetchState::new(reqs, TreeAttributes::CONTENT);
        let mut write_to_local_cache = HashSet::new();
        let mut write_to_memcache = HashSet::new();

        if let Some(ref indexedlog_cache) = self.indexedlog_cache {
            let pending: Vec<_> = common
                .pending(TreeAttributes::CONTENT, false)
                .map(|(key, _attrs)| key.clone())
                .collect();
            for key in pending.into_iter() {
                if let Some(entry) = indexedlog_cache.get_entry(key)? {
                    common.found(entry.key().clone(), LazyTree::IndexedLog(entry).into());
                }
            }
        }

        if let Some(ref indexedlog_local) = self.indexedlog_local {
            let pending: Vec<_> = common
                .pending(TreeAttributes::CONTENT, false)
                .map(|(key, _attrs)| key.clone())
                .collect();
            for key in pending.into_iter() {
                if let Some(entry) = indexedlog_local.get_entry(key)? {
                    common.found(entry.key().clone(), LazyTree::IndexedLog(entry).into());
                }
            }
        }

        if self.use_memcache() {
            if let Some(ref memcache) = self.memcache {
                let pending: Vec<_> = common
                    .pending(TreeAttributes::CONTENT, false)
                    .map(|(key, _attrs)| key.clone())
                    .collect();

                if !pending.is_empty() {
                    for entry in memcache.get_data_iter(&pending)? {
                        let entry = entry?;
                        if self.indexedlog_cache.is_some() && self.cache_to_local_cache {
                            write_to_local_cache.insert(entry.key.clone());
                        }
                        common.found(entry.key.clone(), LazyTree::Memcache(entry).into());
                    }
                }
            }
        }

        if let Some(ref edenapi) = self.edenapi {
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
                let response = edenapi.trees_blocking(pending, None, None)?;
                for entry in response.entries {
                    let entry = entry?;
                    if self.indexedlog_cache.is_some() && self.cache_to_local_cache {
                        write_to_local_cache.insert(entry.key().clone());
                    }
                    if self.memcache.is_some() && self.cache_to_memcache && self.use_memcache() {
                        write_to_memcache.insert(entry.key().clone());
                    }
                    common.found(entry.key().clone(), LazyTree::EdenApi(entry).into());
                }
                util::record_edenapi_stats(&span, &response.stats);
            }
        }

        if let Some(ref contentstore) = self.contentstore {
            let pending: Vec<_> = common
                .pending(TreeAttributes::CONTENT, false)
                .map(|(key, _attrs)| StoreKey::HgId(key.clone()))
                .collect();
            if !pending.is_empty() {
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
                        common.found(key, LazyTree::ContentStore(blob.into(), meta).into());
                    }
                }
            }
        }

        // TODO(meyer): Report incomplete / not found, handle errors better instead of just always failing the batch, etc
        let results = common.results(FetchErrors::new(), ());

        // TODO(meyer): We can do this in the background if we actually want to make this implementation perform well.
        // TODO(meyer): We shouldn't fail the batch on write failures here.
        if self.cache_to_local_cache {
            if let Some(ref indexedlog_cache) = self.indexedlog_cache {
                for key in write_to_local_cache.into_iter() {
                    if let Some(ref content) = results.complete[&key].content {
                        if let Some(entry) = content.indexedlog_cache_entry(key)? {
                            indexedlog_cache.put_entry(entry)?;
                        }
                    }
                }
            }
        }

        if self.cache_to_memcache {
            if let Some(ref memcache) = self.memcache {
                for key in write_to_memcache.into_iter() {
                    if let Some(ref content) = results.complete[&key].content {
                        if let Some(entry) = content.indexedlog_cache_entry(key)? {
                            memcache.add_mcdata(entry.try_into()?);
                        }
                    }
                }
            }
        }

        Ok(results)
    }

    fn use_memcache(&self) -> bool {
        // Only use memcache if the process has been around a while. It takes 2s to setup, which
        // hurts responiveness for short commands.
        self.creation_time.elapsed() > MEMCACHE_DELAY
    }

    fn write_batch(&self, entries: impl Iterator<Item = (Key, Bytes, Metadata)>) -> Result<()> {
        if let Some(ref indexedlog_local) = self.indexedlog_local {
            for (key, bytes, meta) in entries {
                indexedlog_local.put_entry(Entry::new(key, bytes, meta))?;
            }
        }
        Ok(())
    }

    /// Returns a TreeStore with only the local subset of backends
    pub fn local(&self) -> TreeStore {
        TreeStore {
            indexedlog_local: self.indexedlog_local.clone(),
            indexedlog_cache: self.indexedlog_cache.clone(),
            cache_to_local_cache: false,
            memcache: None,
            cache_to_memcache: false,
            edenapi: None,
            contentstore: None,
            creation_time: Instant::now(),
        }
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

            creation_time: Instant::now(),
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
            creation_time: Instant::now(),
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
                .fetch_batch(std::iter::once(key.clone()).filter_map(StoreKey::maybe_into_key))?
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
                .fetch_batch(std::iter::once(key.clone()).filter_map(StoreKey::maybe_into_key))?
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
        if let Some(contentstore) = self.contentstore.as_ref() {
            contentstore.refresh()?;
        }
        self.flush()
    }
}

impl RemoteDataStore for TreeStore {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        Ok(self
            .fetch_batch(keys.iter().cloned().filter_map(StoreKey::maybe_into_key))?
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
