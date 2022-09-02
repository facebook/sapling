/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use configmodel::ConfigExt;
use configparser::config::ConfigSet;
use edenapi::Builder as EdenApiBuilder;
use log::warn;
use manifest::List;
use manifest::Manifest;
use manifest_tree::TreeManifest;
use revisionstore::ContentStore;
use revisionstore::ContentStoreBuilder;
use revisionstore::EdenApiFileStore;
use revisionstore::EdenApiTreeStore;
use revisionstore::HgIdDataStore;
use revisionstore::LegacyStore;
use revisionstore::LocalStore;
use revisionstore::MemcacheStore;
use revisionstore::RemoteDataStore;
use revisionstore::StoreKey;
use revisionstore::StoreResult;
use tracing::event;
use tracing::instrument;
use tracing::Level;
use types::HgId;
use types::Key;
use types::RepoPath;
use types::RepoPathBuf;

use crate::remotestore::FakeRemoteStore;
use crate::treecontentstore::TreeContentStore;
use crate::utils::key_from_path_node_slice;

pub struct BackingContentStores {
    blobstore: ContentStore,
    treestore: Arc<TreeContentStore>,
}

impl BackingContentStores {
    pub fn new(config: &ConfigSet, hg: impl AsRef<Path>, use_edenapi: bool) -> Result<Self> {
        let store_path = hg.as_ref().join("store");

        #[allow(unused_mut)]
        let mut blobstore = ContentStoreBuilder::new(&config).local_path(&store_path);
        let treestore = ContentStoreBuilder::new(&config)
            .local_path(&store_path)
            .suffix(Path::new("manifests"));

        // Memcache takes 30s to initialize on debug builds slowing down tests significantly, let's
        // not even try to initialize it then.
        if !cfg!(debug_assertions) {
            match MemcacheStore::new(config) {
                Ok(memcache) => {
                    // XXX: Add the memcachestore for the treestore.
                    blobstore = blobstore.memcachestore(Arc::new(memcache));
                }
                Err(e) => warn!("couldn't initialize Memcache: {}", e),
            }
        }

        let (blobstore, treestore) = match config.get_opt::<String>("remotefilelog", "reponame")? {
            Some(_repo) if use_edenapi => {
                let edenapi = EdenApiBuilder::from_config(config)?.build()?;
                let fileremotestore = EdenApiFileStore::new(edenapi.clone());
                let treeremotestore = EdenApiTreeStore::new(edenapi);
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
        self.get_blob_by_key(key, local_only)
    }

    #[instrument(level = "debug", skip(self))]
    fn get_blob_by_key(&self, key: Key, local_only: bool) -> Result<Option<Vec<u8>>> {
        // check if the blob present on disk
        if local_only && !self.blobstore.contains(&StoreKey::from(&key))? {
            event!(Level::DEBUG, "blob not found locally");
            return Ok(None);
        }

        self.get_blob_impl(key)
    }

    /// Fetch file contents in batch. Whenever a blob is fetched, the supplied `resolve` function is
    /// called with the file content or an error message, and the index of the blob in the request
    /// array. When `local_only` is enabled, this function will only check local disk for the file
    /// content.
    #[instrument(level = "debug", skip(self, resolve))]
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

    fn get_tree_impl(&self, node: HgId) -> Result<List> {
        let manifest = TreeManifest::durable(self.treestore.clone(), node);
        // Since node is referring to the tree we're looking for, pass an empty path.
        manifest.list(RepoPath::empty())
    }

    #[instrument(level = "debug", skip(self))]
    pub fn get_tree(&self, node: &[u8], local_only: bool) -> Result<Option<List>> {
        let node = HgId::from_slice(node)?;
        if local_only {
            let path = RepoPathBuf::new();
            let key = Key::new(path, node);
            // check if the blob is present on disk
            if !self
                .treestore
                .as_content_store()
                .contains(&StoreKey::from(&key))?
            {
                event!(Level::DEBUG, "tree not found locally");
                return Ok(None);
            }
        }

        Ok(Some(self.get_tree_impl(node)?))
    }

    /// Fetch tree contents in batch. Whenever a tree is fetched, the supplied `resolve` function is
    /// called with the tree content or an error message, and the index of the tree in the request
    /// array. When `local_only` is enabled, this function will only check local disk for the file
    /// content.
    #[instrument(level = "debug", skip(self, resolve))]
    pub fn get_tree_batch<F>(&self, keys: Vec<Result<Key>>, local_only: bool, resolve: F)
    where
        F: Fn(usize, Result<Option<List>>) -> (),
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

        let contentstore = self.treestore.as_content_store();
        for (index, key) in requests {
            let store_key = StoreKey::from(&key);
            // Assuming a blob do not exist if `.contains` call fails
            if contentstore.contains(&store_key).unwrap_or(false) {
                resolve(index, Some(self.get_tree_impl(key.hgid)).transpose())
            } else if !local_only {
                missing.push(store_key);
                missing_requests.push((index, key));
            }
        }

        // If this is a local only read, nothing else we can do.
        if local_only {
            return;
        }

        let _ = contentstore.prefetch(&missing);
        for (index, key) in missing_requests {
            resolve(index, Some(self.get_tree_impl(key.hgid)).transpose())
        }
    }

    /// Forces backing store to rescan pack files or local indexes
    #[instrument(level = "debug", skip(self))]
    pub fn flush(&self) {
        self.blobstore.refresh().ok();
        self.treestore.as_content_store().refresh().ok();
    }
}
