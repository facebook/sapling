/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;

use anyhow::Result;
use anyhow::anyhow;
use blob::Blob;
use metrics::Counter;
use revisionstore::scmstore::FileAuxData;
use storemodel::BoxIterator;
use storemodel::Bytes;
use storemodel::FileStore;
use storemodel::InsertOpts;
use storemodel::KeyStore;
use storemodel::SerializationFormat;
use storemodel::minibytes;
use types::FetchContext;
use types::HgId;
use types::Key;
use types::RepoPath;

use crate::caching::CachingKeyStore;

static CACHE_HITS: Counter = Counter::new_counter("filestore.cache.hits");
static CACHE_REQS: Counter = Counter::new_counter("filestore.cache.reqs");

// FileStore wrapper which caches trees in an LRU cache.
#[derive(Clone)]
pub(crate) struct CachingFileStore {
    key_store: Arc<CachingKeyStore>,
    store: Arc<dyn FileStore>,
}

impl CachingFileStore {
    pub fn new(store: Arc<dyn FileStore>, size: usize) -> Self {
        Self {
            key_store: CachingKeyStore::new(store.clone_key_store().into(), size).into(),
            store: store.clone(),
        }
    }

    /// Fetch a single item from cache.
    fn cached_single(&self, id: &HgId) -> Option<Bytes> {
        CACHE_REQS.increment();
        let result = self.key_store.cached_single(id);
        if result.is_some() {
            CACHE_HITS.increment();
        }
        result
    }

    /// Fetch multiple items from cache, returning (misses, hits).
    fn cached_multi(&self, keys: Vec<Key>) -> (Vec<Key>, Vec<(Key, Bytes)>) {
        CACHE_REQS.add(keys.len());
        let found = self.key_store.cached_multi(keys);
        CACHE_HITS.add(found.1.len());
        found
    }

    /// Insert a (key, value) pair into the cache.
    /// Note: this does not insert the value into the underlying store
    fn cache_with_key(&self, key: HgId, data: Bytes) -> Result<()> {
        self.key_store.cache_with_key(key, data)
    }

    pub fn format(&self) -> SerializationFormat {
        self.store.format()
    }
}

impl KeyStore for CachingFileStore {
    fn get_content_iter(
        &self,
        fctx: FetchContext,
        keys: Vec<Key>,
    ) -> Result<BoxIterator<Result<(Key, Blob)>>> {
        self.key_store.get_content_iter(fctx, keys)
    }

    fn get_local_content(&self, path: &RepoPath, hgid: HgId) -> Result<Option<Blob>> {
        self.key_store.get_local_content(path, hgid)
    }

    fn get_content(&self, fctx: FetchContext, path: &RepoPath, hgid: HgId) -> Result<Blob> {
        self.key_store.get_content(fctx, path, hgid)
    }

    fn prefetch(&self, keys: Vec<Key>) -> Result<()> {
        self.key_store.prefetch(keys)
    }

    fn insert_data(&self, opts: InsertOpts, path: &RepoPath, data: &[u8]) -> Result<HgId> {
        self.key_store.insert_data(opts, path, data)
    }

    fn flush(&self) -> Result<()> {
        self.key_store.flush()
    }

    fn refresh(&self) -> Result<()> {
        self.key_store.refresh()
    }

    fn format(&self) -> SerializationFormat {
        self.key_store.format()
    }

    fn statistics(&self) -> Vec<(String, usize)> {
        self.store.statistics()
    }

    fn clone_key_store(&self) -> Box<dyn KeyStore> {
        self.store.clone_key_store()
    }
}

// Our caching is not aux aware, so just proxy all the higher level file methods directly
// to wrapped FileStore.
impl FileStore for CachingFileStore {
    fn get_rename_iter(
        &self,
        keys: Vec<Key>,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(Key, Key)>>> {
        self.store.get_rename_iter(keys)
    }

    fn get_local_aux(&self, path: &RepoPath, id: HgId) -> anyhow::Result<Option<FileAuxData>> {
        self.store.get_local_aux(path, id)
    }

    fn get_aux_iter(
        &self,
        fctx: FetchContext,
        keys: Vec<Key>,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(Key, FileAuxData)>>> {
        self.store.get_aux_iter(fctx, keys)
    }

    /// Get auxiliary metadata for the given file.
    /// Contact remote server on demand. Might block.
    /// When fetching many files, use `get_aux_iter` instead of calling this in a loop.
    fn get_aux(
        &self,
        fctx: FetchContext,
        path: &RepoPath,
        id: HgId,
    ) -> anyhow::Result<FileAuxData> {
        self.store.get_aux(fctx, path, id)
    }

    fn get_hg_parents(&self, _path: &RepoPath, _id: HgId) -> anyhow::Result<Vec<HgId>> {
        Err(anyhow!(
            "CachingFileStore does not implement legacy FileStore trait methods."
        ))
    }

    fn get_hg_raw_content(&self, _path: &RepoPath, _id: HgId) -> anyhow::Result<minibytes::Bytes> {
        Err(anyhow!(
            "CachingFileStore does not implement legacy FileStore trait methods."
        ))
    }

    fn get_hg_flags(&self, _path: &RepoPath, _id: HgId) -> anyhow::Result<u32> {
        Err(anyhow!(
            "CachingFileStore does not implement legacy FileStore trait methods."
        ))
    }

    /// Upload LFS files specified by the keys.
    /// This is called before push.
    fn upload_lfs(&self, _keys: Vec<Key>) -> anyhow::Result<()> {
        Err(anyhow!(
            "CachingFileStore does not implement LFS trait methods."
        ))
    }

    fn insert_file(&self, opts: InsertOpts, path: &RepoPath, data: &[u8]) -> anyhow::Result<HgId> {
        self.store.insert_file(opts, path, data)
    }

    fn as_key_store(&self) -> &dyn storemodel::KeyStore
    where
        Self: Sized,
    {
        self
    }

    fn clone_file_store(&self) -> Box<dyn FileStore> {
        self.store.clone_file_store()
    }
}
