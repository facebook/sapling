/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::remotestore::FakeRemoteStore;
use crate::treecontentstore::TreeContentStore;
use crate::utils::key_from_path_node_slice;
use anyhow::Result;
use configparser::config::ConfigSet;
use configparser::hg::ConfigSetHgExt;
use edenapi::{Builder as EdenApiBuilder, EdenApi};
use log::warn;
use manifest::{List, Manifest};
use manifest_tree::TreeManifest;
use revisionstore::{
    ContentStore, ContentStoreBuilder, EdenApiFileStore, EdenApiTreeStore, HgIdDataStore,
    LocalStore, MemcacheStore, RemoteDataStore, StoreKey, StoreResult,
};
use std::path::Path;
use std::sync::Arc;
use types::{Key, Node, RepoPath};

pub struct BackingStore {
    blobstore: ContentStore,
    treestore: Arc<TreeContentStore>,
}

impl BackingStore {
    pub fn new<P: AsRef<Path>>(repository: P, use_edenapi: bool) -> Result<Self> {
        let hg = repository.as_ref().join(".hg");
        let mut config = ConfigSet::new();
        config.load_system();
        config.load_user();
        config.load_hgrc(hg.join("hgrc"), "repository");

        let store_path = hg.join("store");
        let mut blobstore = ContentStoreBuilder::new(&config).local_path(&store_path);
        let treestore = ContentStoreBuilder::new(&config)
            .local_path(&store_path)
            .suffix(Path::new("manifests"));

        match MemcacheStore::new(&config) {
            Ok(memcache) => {
                // XXX: Add the memcachestore for the treestore.
                blobstore = blobstore.memcachestore(Arc::new(memcache));
            }
            Err(e) => warn!("couldn't initialize Memcache: {}", e),
        }

        let (blobstore, treestore) = match config.get_opt::<String>("remotefilelog", "reponame")? {
            Some(repo) if use_edenapi => {
                let edenapi = EdenApiBuilder::from_config(&config)?.build()?;
                let edenapi: Arc<dyn EdenApi> = Arc::new(edenapi);
                let fileremotestore = EdenApiFileStore::new(repo.clone(), edenapi.clone())?;
                let treeremotestore = EdenApiTreeStore::new(repo, edenapi)?;
                (
                    blobstore.remotestore(fileremotestore).build()?,
                    treestore.remotestore(treeremotestore).build()?,
                )
            }
            _ => (
                blobstore.remotestore(Arc::new(FakeRemoteStore)).build()?,
                treestore.remotestore(Arc::new(FakeRemoteStore)).build()?,
            ),
        };

        Ok(Self {
            blobstore,
            treestore: Arc::new(TreeContentStore::new(treestore)),
        })
    }

    fn get_blob_impl(&self, key: Key) -> Result<Option<Vec<u8>>> {
        // Return None for LFS blobs
        // TODO: LFS support
        if let Ok(StoreResult::Found(metadata)) =
            self.blobstore.get_meta(StoreKey::hgid(key.clone()))
        {
            if metadata.is_lfs() {
                return Ok(None);
            }
        }

        Ok(self
            .blobstore
            .get_file_content(&key)?
            .map(|blob| blob.as_ref().to_vec()))
    }

    /// Reads file from blobstores. When `local_only` is true, this function will only read blobs
    /// from on disk stores.
    pub fn get_blob(&self, path: &[u8], node: &[u8], local_only: bool) -> Result<Option<Vec<u8>>> {
        let key = key_from_path_node_slice(path, node)?;

        // check if the blob present on disk
        if local_only && !self.blobstore.contains(&StoreKey::from(&key))? {
            return Ok(None);
        }

        self.get_blob_impl(key)
    }

    /// Fetch file contents in batch. Whenever a blob is fetched, the supplied `resolve` function is
    /// called with the file content or an error message, and the index of the blob in the request
    /// array. When `local_only` is enabled, this function will only check local disk for the file
    /// content.
    pub fn get_blob_batch<F>(&self, keys: Vec<Result<Key>>, local_only: bool, resolve: F)
    where
        F: Fn(usize, Result<Option<Vec<u8>>>) -> (),
    {
        // logic:
        // 1. convert all path & nodes into `StoreKey`
        // 2. try to resolve blobs that are already local
        // 3. fetch anything that is not local and refetch
        let requests = keys
            .into_iter()
            .enumerate()
            .filter_map(|(index, key)| match key {
                Ok(key) => Some((index, key)),
                Err(e) => {
                    // return early when the key is invalid
                    resolve(index, Err(e));
                    None
                }
            });

        let mut missing = Vec::new();
        let mut missing_requests = Vec::new();

        for (index, key) in requests {
            let store_key = StoreKey::from(&key);
            // Assuming a blob do not exist if `.contains` call fails
            if self.blobstore.contains(&store_key).unwrap_or(false) {
                resolve(index, self.get_blob_impl(key))
            } else if !local_only {
                missing.push(store_key);
                missing_requests.push((index, key));
            }
        }

        // If this is a local only read, nothing else we can do.
        if local_only {
            return;
        }

        let _ = self.blobstore.prefetch(&missing);
        for (index, key) in missing_requests {
            resolve(index, self.get_blob_impl(key))
        }
    }

    pub fn get_tree(&self, node: &[u8]) -> Result<List> {
        let node = Node::from_slice(node)?;
        let manifest = TreeManifest::durable(self.treestore.clone(), node);

        manifest.list(RepoPath::empty())
    }

    /// forces backing store to rescan pack files
    pub fn refresh(&self) {
        self.blobstore.get_missing(&[]).ok();
        self.treestore.as_content_store().get_missing(&[]).ok();
    }
}
