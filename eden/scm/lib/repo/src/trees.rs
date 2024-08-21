/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::bail;
use anyhow::Result;
use commits_trait::DagCommits;
use lru_cache::LruCache;
use manifest_tree::ReadTreeManifest;
use manifest_tree::TreeManifest;
use manifest_tree::TreeStore;
use metrics::Counter;
use parking_lot::Mutex;
use parking_lot::RwLock;
use storemodel::BoxIterator;
use storemodel::Bytes;
use storemodel::InsertOpts;
use storemodel::KeyStore;
use storemodel::SerializationFormat;
use storemodel::TreeAuxData;
use storemodel::TreeEntry;
use types::fetch_mode::FetchMode;
use types::hgid;
use types::HgId;
use types::Key;
use types::RepoPath;

pub struct TreeManifestResolver {
    dag_commits: Arc<RwLock<Box<dyn DagCommits + Send + 'static>>>,
    tree_store: Arc<dyn TreeStore>,
}

impl TreeManifestResolver {
    pub fn new(
        dag_commits: Arc<RwLock<Box<dyn DagCommits + Send + 'static>>>,
        tree_store: Arc<dyn TreeStore>,
    ) -> Self {
        TreeManifestResolver {
            dag_commits,
            tree_store,
        }
    }
}

impl ReadTreeManifest for TreeManifestResolver {
    fn get(&self, commit_id: &HgId) -> Result<TreeManifest> {
        if commit_id.is_null() {
            // Null commit represents a working copy with no parents. Avoid
            // querying the backend since this is not a real commit.
            return Ok(TreeManifest::ephemeral(self.tree_store.clone()));
        }

        Ok(TreeManifest::durable(
            self.tree_store.clone(),
            self.get_root_id(commit_id)?,
        ))
    }

    fn get_root_id(&self, commit_id: &HgId) -> Result<HgId> {
        if commit_id.is_null() {
            // Special case: null commit's manifest node is null.
            return Ok(hgid::NULL_ID);
        }

        let commit_store = self.dag_commits.read().to_dyn_read_root_tree_ids();
        let tree_ids =
            async_runtime::block_on(commit_store.read_root_tree_ids(vec![commit_id.clone()]))?;

        if tree_ids.is_empty() {
            bail!("no root tree id for commit {commit_id}");
        }

        Ok(tree_ids[0].1)
    }
}

static CACHE_HITS: Counter = Counter::new_counter("treeresolver.cache.hits");
static CACHE_REQS: Counter = Counter::new_counter("treeresolver.cache.reqs");

// TreeStore wrapper which caches trees in an LRU cache.
pub(crate) struct CachingTreeStore {
    store: Arc<dyn TreeStore>,
    cache: Arc<Mutex<LruCache<HgId, Bytes>>>,
}

impl CachingTreeStore {
    pub fn new(store: Arc<dyn TreeStore>, size: usize) -> Self {
        Self {
            store,
            cache: Arc::new(Mutex::new(LruCache::new(size))),
        }
    }

    // Fetch a single item from cache.
    fn cached_single(&self, id: &HgId) -> Option<Bytes> {
        CACHE_REQS.add(1);

        let cached = self.cache.lock().get_mut(id).cloned();
        if cached.is_some() {
            CACHE_HITS.add(1);
        }
        cached
    }

    // Fetch multiple items from cache, returning (misses, hits).
    fn cached_multi(&self, mut keys: Vec<Key>) -> (Vec<Key>, Vec<(Key, Bytes)>) {
        CACHE_REQS.add(keys.len());

        let mut cache = self.cache.lock();

        let mut found = Vec::new();
        keys.retain(|key| {
            if let Some(data) = cache.get_mut(&key.hgid) {
                found.push((key.clone(), data.clone()));
                false
            } else {
                true
            }
        });

        CACHE_HITS.add(found.len());

        (keys, found)
    }
}

// Our caching is not aux aware, so just proxy all the higher level tree methods directly
// to wrapped TreeStore.
impl TreeStore for CachingTreeStore {
    fn get_remote_tree_iter(
        &self,
        keys: Vec<Key>,
    ) -> Result<BoxIterator<Result<(Key, Box<dyn TreeEntry>)>>> {
        self.store.get_remote_tree_iter(keys)
    }

    fn get_tree_iter(
        &self,
        keys: Vec<Key>,
        fetch_mode: FetchMode,
    ) -> Result<BoxIterator<Result<(Key, Box<dyn TreeEntry>)>>> {
        self.store.get_tree_iter(keys, fetch_mode)
    }

    fn get_tree_aux_data_iter(
        &self,
        keys: Vec<Key>,
        fetch_mode: FetchMode,
    ) -> Result<BoxIterator<Result<(Key, TreeAuxData)>>> {
        self.store.get_tree_aux_data_iter(keys, fetch_mode)
    }

    fn get_local_tree_aux_data(&self, path: &RepoPath, id: HgId) -> Result<Option<TreeAuxData>> {
        self.store.get_local_tree_aux_data(path, id)
    }

    fn get_tree_aux_data(
        &self,
        path: &RepoPath,
        id: HgId,
        fetch_mode: FetchMode,
    ) -> Result<TreeAuxData> {
        self.store.get_tree_aux_data(path, id, fetch_mode)
    }
}

impl KeyStore for CachingTreeStore {
    fn get_content_iter(
        &self,
        keys: Vec<Key>,
        fetch_mode: FetchMode,
    ) -> Result<BoxIterator<Result<(Key, Bytes)>>> {
        let (keys, cached) = self.cached_multi(keys);

        let uncached = CachingIter {
            iter: self.store.get_content_iter(keys, fetch_mode)?,
            cache: self.cache.clone(),
        };

        Ok(Box::new(uncached.chain(cached.into_iter().map(Ok))))
    }

    fn get_local_content(&self, path: &RepoPath, hgid: HgId) -> Result<Option<Bytes>> {
        if let Some(cached) = self.cached_single(&hgid) {
            Ok(Some(cached))
        } else {
            match self.store.get_local_content(path, hgid) {
                Ok(Some(data)) => {
                    self.cache.lock().insert(hgid, data.clone());
                    Ok(Some(data))
                }
                r => r,
            }
        }
    }

    fn get_content(&self, path: &RepoPath, hgid: HgId, fetch_mode: FetchMode) -> Result<Bytes> {
        if let Some(cached) = self.cached_single(&hgid) {
            Ok(cached)
        } else {
            match self.store.get_content(path, hgid, fetch_mode) {
                Ok(data) => {
                    self.cache.lock().insert(hgid, data.clone());
                    Ok(data)
                }
                r => r,
            }
        }
    }

    fn prefetch(&self, keys: Vec<Key>) -> Result<()> {
        // Intercept prefetch() so we can prime our cache. This is what manifest-tree
        // operations like bfs_iter and diff use when walking trees.
        self.get_content_iter(keys, FetchMode::AllowRemote)?
            .for_each(|_| ());
        Ok(())
    }

    fn insert_data(&self, opts: InsertOpts, path: &RepoPath, data: &[u8]) -> Result<HgId> {
        self.store.insert_data(opts, path, data)
    }

    fn flush(&self) -> Result<()> {
        self.store.flush()
    }

    fn refresh(&self) -> Result<()> {
        self.store.refresh()
    }

    fn format(&self) -> SerializationFormat {
        self.store.format()
    }

    fn statistics(&self) -> Vec<(String, usize)> {
        self.store.statistics()
    }
}

// An Iterator that lazily populates tree cache during iteration.
struct CachingIter<'a> {
    iter: BoxIterator<'a, Result<(Key, Bytes)>>,
    cache: Arc<Mutex<LruCache<HgId, Bytes>>>,
}

impl<'a> Iterator for CachingIter<'a> {
    type Item = Result<(Key, Bytes)>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.iter.next() {
            Some(item) => {
                if let Ok((key, data)) = &item {
                    self.cache.lock().insert(key.hgid, data.clone());
                }
                Some(item)
            }
            None => None,
        }
    }
}

#[cfg(test)]
mod test {
    use manifest_tree::testutil::TestStore;
    use types::RepoPathBuf;

    use super::*;

    #[test]
    fn test_tree_cache() -> Result<()> {
        let inner_store = Arc::new(TestStore::new());

        let caching_store = CachingTreeStore {
            store: inner_store.clone(),
            cache: Arc::new(Mutex::new(LruCache::new(5))),
        };

        let dir1_path = RepoPathBuf::from_string("dir1".to_string())?;
        let dir2_path = RepoPathBuf::from_string("dir2".to_string())?;

        let dir1_id = caching_store.insert_data(Default::default(), &dir1_path, b"dir1")?;
        let dir2_id = caching_store.insert_data(Default::default(), &dir2_path, b"dir2")?;

        assert_eq!(inner_store.key_fetch_count(), 0);

        assert_eq!(
            caching_store.get_content(&dir1_path, dir1_id, FetchMode::AllowRemote)?,
            b"dir1"
        );
        assert_eq!(inner_store.key_fetch_count(), 1);

        // Fetch again - make sure we cached it.
        assert_eq!(
            caching_store.get_content(&dir1_path, dir1_id, FetchMode::AllowRemote)?,
            b"dir1"
        );
        assert_eq!(inner_store.key_fetch_count(), 1);

        // Fetch both via iterator.
        let key1 = Key::new(dir1_path.clone(), dir1_id);
        let key2 = Key::new(dir2_path.clone(), dir2_id);
        assert_eq!(
            caching_store
                .get_content_iter(vec![key1.clone(), key2.clone()], FetchMode::AllowRemote)?
                .collect::<Result<Vec<_>>>()?,
            vec![
                (key2.clone(), b"dir2".as_ref().into()),
                (key1.clone(), b"dir1".as_ref().into()),
            ]
        );
        // Should only have done 1 additional fetch for dir2.
        assert_eq!(inner_store.key_fetch_count(), 2);

        assert_eq!(
            caching_store
                .get_content_iter(vec![key1.clone(), key2.clone()], FetchMode::AllowRemote)?
                .collect::<Result<Vec<_>>>()?,
            vec![
                (key1.clone(), b"dir1".as_ref().into()),
                (key2.clone(), b"dir2".as_ref().into()),
            ]
        );

        caching_store.prefetch(vec![key1.clone(), key2.clone()])?;

        assert_eq!(inner_store.key_fetch_count(), 2);

        Ok(())
    }
}
