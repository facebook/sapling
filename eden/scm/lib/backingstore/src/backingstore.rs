/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::treecontentstore::TreeContentStore;
use anyhow::Result;
use configparser::config::ConfigSet;
use configparser::hg::ConfigSetHgExt;
use edenapi::{EdenApi, EdenApiCurlClient};
use log::warn;
use manifest::{List, Manifest};
use manifest_tree::TreeManifest;
use revisionstore::{
    ContentStore, ContentStoreBuilder, EdenApiHgIdRemoteStore, HgIdDataStore, LocalStore,
    MemcacheStore, StoreKey,
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

        let (blobstore, treestore) = if use_edenapi {
            let edenapi_config = edenapi::Config::from_hg_config(&config)?;
            let edenapi = EdenApiCurlClient::new(edenapi_config)?;
            let edenapi: Arc<dyn EdenApi> = Arc::new(edenapi);
            let fileremotestore = Arc::new(EdenApiHgIdRemoteStore::filestore(edenapi.clone()));
            let treeremotestore = Arc::new(EdenApiHgIdRemoteStore::treestore(edenapi));

            (
                blobstore.remotestore(fileremotestore).build()?,
                treestore.remotestore(treeremotestore).build()?,
            )
        } else {
            (blobstore.build()?, treestore.build()?)
        };

        Ok(Self {
            blobstore,
            treestore: Arc::new(TreeContentStore::new(treestore)),
        })
    }

    /// Reads file from blobstores. When `local_only` is true, this function will only read blobs
    /// from on disk stores.
    pub fn get_blob(&self, path: &[u8], node: &[u8], local_only: bool) -> Result<Option<Vec<u8>>> {
        let path = RepoPath::from_utf8(path)?.to_owned();
        let node = Node::from_slice(node)?;
        let key = Key::new(path, node);

        // check if the blob present on disk
        if local_only && self.blobstore.contains(&StoreKey::from(&key))? {
            return Ok(None);
        }

        // Return None for LFS blobs
        // TODO: LFS support
        if let Ok(Some(metadata)) = self.blobstore.get_meta(&key) {
            if metadata.is_lfs() {
                return Ok(None);
            }
        }

        Ok(self
            .blobstore
            .get_file_content(&key)?
            .map(|blob| blob.as_ref().to_vec()))
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
