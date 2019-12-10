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
use manifest::{List, Manifest};
use manifest_tree::TreeManifest;
use revisionstore::{ContentStore, ContentStoreBuilder, DataStore, EdenApiRemoteStore};
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
        let blobstore = ContentStoreBuilder::new(&store_path, &config);
        let treestore =
            ContentStoreBuilder::new(&store_path, &config).suffix(Path::new("manifests"));

        let (blobstore, treestore) = if use_edenapi {
            let edenapi_config = edenapi::Config::from_hg_config(&config)?;
            let edenapi = Box::new(EdenApiCurlClient::new(edenapi_config)?);
            let edenapi: Arc<Box<(dyn EdenApi)>> = Arc::new(edenapi);
            let fileremotestore = Box::new(EdenApiRemoteStore::filestore(edenapi.clone()));
            let treeremotestore = Box::new(EdenApiRemoteStore::treestore(edenapi));

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

    pub fn get_blob(&self, path: &[u8], node: &[u8]) -> Result<Option<Vec<u8>>> {
        let path = RepoPath::from_utf8(path)?.to_owned();
        let node = Node::from_slice(node)?;
        let key = Key::new(path, node);

        // Return None for LFS blobs
        // TODO: LFS support
        if let Ok(Some(metadata)) = self.blobstore.get_meta(&key) {
            if let Some(flag) = metadata.flags {
                if flag == 0x2000 {
                    return Ok(None);
                }
            }
        }

        self.blobstore
            .get(&key)
            .map(|blob| blob.map(discard_metadata_header))
    }

    pub fn get_tree(&self, node: &[u8]) -> Result<List> {
        let node = Node::from_slice(node)?;
        let manifest = TreeManifest::durable(self.treestore.clone(), node);

        manifest.list(RepoPath::empty())
    }
}

/// Removes the possible metadata header at the beginning of a blob.
///
/// The metadata header is defined as the block surrounded by '\x01\x0A' at the beginning of the
/// blob. If there is no closing tag found in the blob, this function will simply return the
/// original blob.
///
/// See `edenscm/mercurial/filelog.py` for the Python implementation.
fn discard_metadata_header(data: Vec<u8>) -> Vec<u8> {
    // Returns when the blob less than 2 bytes long or no metadata header starting tag at the
    // beginning
    if data.len() < 2 || !(data[0] == 0x01 && data[1] == 0x0A) {
        return data;
    }

    // Finds the position of the closing tag
    let closing_tag = data.windows(2).skip(2).position(|bytes| match *bytes {
        [a, b] => a == 0x01 && b == 0x0A,
        // Rust cannot infer that `.windows` only gives us a two-element slice so we have to write
        // the predicate this way to provide the default clause.
        _ => false,
    });

    if let Some(idx) = closing_tag {
        // Skip two bytes for the starting tag and two bytes for the closing tag
        data.into_iter().skip(2 + idx + 2).collect()
    } else {
        data
    }
}

#[test]
fn test_discard_metadata_header() {
    assert_eq!(discard_metadata_header(vec![]), Vec::<u8>::new());
    assert_eq!(discard_metadata_header(vec![0x1]), vec![0x1]);
    assert_eq!(discard_metadata_header(vec![0x1, 0x1]), vec![0x1, 0x1]);
    assert_eq!(discard_metadata_header(vec![0x1, 0xA]), vec![0x1, 0xA]);

    // Empty metadata header and empty blob
    assert_eq!(
        discard_metadata_header(vec![0x1, 0xA, 0x1, 0xA]),
        Vec::<u8>::new()
    );
    // Metadata header with some data but empty blob
    assert_eq!(
        discard_metadata_header(vec![0x1, 0xA, 0xA, 0xB, 0xC, 0x1, 0xA]),
        Vec::<u8>::new()
    );
    // Metadata header with data and blob
    assert_eq!(
        discard_metadata_header(vec![0x1, 0xA, 0xA, 0xB, 0xC, 0x1, 0xA, 0xA, 0xB, 0xC]),
        vec![0xA, 0xB, 0xC]
    );
}
