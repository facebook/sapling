/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;

use anyhow::Result;
use anyhow::bail;
use blob::Blob;
use commits_trait::DagCommits;
use manifest_tree::ReadTreeManifest;
use manifest_tree::TreeManifest;
use manifest_tree::TreeStore;
use metrics::Counter;
use parking_lot::RwLock;
use storemodel::BoxIterator;
use storemodel::Bytes;
use storemodel::InsertOpts;
use storemodel::KeyStore;
use storemodel::SerializationFormat;
use storemodel::TreeAuxData;
use storemodel::TreeEntry;
use types::FetchContext;
use types::HgId;
use types::Key;
use types::RepoPath;
use types::fetch_mode::FetchMode;
use types::hgid;

use crate::caching::CachingKeyStore;

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

static CACHE_HITS: Counter = Counter::new_counter("treestore.cache.hits");
static CACHE_REQS: Counter = Counter::new_counter("treestore.cache.reqs");

// TreeStore wrapper which caches trees in an LRU cache.
#[derive(Clone)]
pub(crate) struct CachingTreeStore {
    key_store: Arc<CachingKeyStore>,
    store: Arc<dyn TreeStore>,
}

impl CachingTreeStore {
    pub fn new(store: Arc<dyn TreeStore>, size: usize) -> Self {
        Self {
            key_store: CachingKeyStore::new(store.clone_key_store().into(), size).into(),
            store: store.clone(),
        }
    }

    /// Fetch a single item from cache.
    fn cached_single(&self, id: &HgId) -> Option<Bytes> {
        CACHE_REQS.add(1);
        let result = self.key_store.cached_single(id);
        if result.is_some() {
            CACHE_HITS.add(1);
        }
        result
    }

    /// Fetch multiple items from cache, returning (misses, hits).
    fn cached_multi(&self, keys: Vec<Key>) -> (Vec<Key>, Vec<(Key, Bytes)>) {
        CACHE_REQS.add(keys.len());
        let found = self.key_store.cached_multi(keys);
        CACHE_HITS.add(found.0.len());
        found
    }

    /// Insert a (key, value) pair into the cache.
    /// Note: this does not insert the value into the underlying store
    fn cache_with_key(&self, key: HgId, data: Bytes) -> Result<()> {
        self.key_store.cache_with_key(key, data)
    }
}

impl KeyStore for CachingTreeStore {
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
        fctx: FetchContext,
        keys: Vec<Key>,
    ) -> Result<BoxIterator<Result<(Key, Box<dyn TreeEntry>)>>> {
        self.store.get_tree_iter(fctx, keys)
    }

    fn get_tree_aux_data_iter(
        &self,
        fctx: FetchContext,
        keys: Vec<Key>,
    ) -> Result<BoxIterator<Result<(Key, TreeAuxData)>>> {
        self.store.get_tree_aux_data_iter(fctx, keys)
    }

    fn get_local_tree_aux_data(&self, path: &RepoPath, id: HgId) -> Result<Option<TreeAuxData>> {
        self.store.get_local_tree_aux_data(path, id)
    }

    fn get_tree_aux_data(
        &self,
        fctx: FetchContext,
        path: &RepoPath,
        id: HgId,
    ) -> Result<TreeAuxData> {
        self.store.get_tree_aux_data(fctx, path, id)
    }

    fn clone_tree_store(&self) -> Box<dyn TreeStore> {
        self.store.clone_tree_store()
    }
}

/// Tests that only exercise CachingKeyStore code-paths should go in the CachingKeyStore module.
/// This test module is specifically for TreeStore tests.
#[cfg(test)]
mod test {
    use manifest_tree::init;
    use manifest_tree::testutil::TestStore;
    use rand_chacha::ChaChaRng;
    use rand_chacha::rand_core::SeedableRng;
    use storemodel::Kind;
    use storemodel::TreeItemFlag;
    use storemodel::basic_serialize_tree;
    use types::RepoPathBuf;

    use super::*;

    #[test]
    fn test_tree_cache() -> Result<()> {
        init();
        let inner_store = Arc::new(TestStore::new());

        let caching_store = CachingTreeStore::new(inner_store.clone(), 5);

        let top_level_path = RepoPathBuf::from_string("dir1".to_string()).expect("to create path");
        let dir2_path = RepoPathBuf::from_string("dir1/dir2".to_string()).expect("to create path");
        let dir3_path = RepoPathBuf::from_string("dir1/dir3".to_string()).expect("to create path");

        // The ID of the cached trees doesn't actually matter
        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let dir3_id = HgId::random(&mut rng);
        let top_level_id = HgId::random(&mut rng);

        // Insert a tree into the underlying store
        let dir2_id = caching_store
            .insert_tree(
                InsertOpts {
                    kind: Kind::Tree,
                    ..Default::default()
                },
                &dir2_path,
                vec![],
            )
            .expect("to create id");

        // Insert two trees into the cache (not the underlying store).
        let dir3_data = basic_serialize_tree(vec![], caching_store.format())?;
        let top_level_data = basic_serialize_tree(
            vec![
                (
                    dir2_path.clone().last_component().unwrap().to_owned(),
                    dir2_id,
                    TreeItemFlag::Directory,
                ),
                (
                    dir3_path.clone().last_component().unwrap().to_owned(),
                    dir3_id,
                    TreeItemFlag::Directory,
                ),
            ],
            caching_store.format(),
        )?;
        caching_store
            .cache_with_key(dir3_id, dir3_data)
            .expect("to create id");
        caching_store
            .cache_with_key(top_level_id, top_level_data.clone())
            .expect("to create id");

        let trees = caching_store.get_tree_iter(
            FetchContext::new(FetchMode::LocalOnly),
            vec![
                Key::new(dir2_path.clone(), dir2_id),
                Key::new(top_level_path.clone(), top_level_id),
            ],
        )?;

        // TreeStore methods will only contain results from the underlying store. Any cached trees
        // will not be returned.
        for tree in trees {
            match tree {
                Ok(x) => {
                    assert_eq!(dir2_id, x.0.hgid);
                    assert_eq!(dir2_path, x.0.path);
                }
                Err(e) => {
                    e.to_string().contains(top_level_id.to_string().as_str());
                }
            }
        }

        Ok(())
    }
}
