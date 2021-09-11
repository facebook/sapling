/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod contentstores;
mod scmstores;

use std::path::Path;

use anyhow::Result;

use manifest::List;
use types::Key;

use crate::backingstore::{contentstores::BackingContentStores, scmstores::BackingScmStores};

pub enum BackingStore {
    Old(BackingContentStores),
    New(BackingScmStores),
}

use BackingStore::*;

impl BackingStore {
    pub fn new<P: AsRef<Path>>(repository: P, use_edenapi: bool) -> Result<Self> {
        let hg = repository.as_ref().join(".hg");
        let config = configparser::hg::load::<String, String>(Some(&hg), None)?;

        Ok(if config.get_or_default("scmstore", "backingstore")? {
            New(BackingScmStores::new(&config, &hg, use_edenapi)?)
        } else {
            Old(BackingContentStores::new(&config, &hg, use_edenapi)?)
        })
    }

    /// Reads file from blobstores. When `local_only` is true, this function will only read blobs
    /// from on disk stores.
    pub fn get_blob(&self, path: &[u8], node: &[u8], local_only: bool) -> Result<Option<Vec<u8>>> {
        match self {
            Old(stores) => stores.get_blob(path, node, local_only),
            New(stores) => stores.get_blob(path, node, local_only),
        }
    }

    /// Fetch file contents in batch. Whenever a blob is fetched, the supplied `resolve` function is
    /// called with the file content or an error message, and the index of the blob in the request
    /// array. When `local_only` is enabled, this function will only check local disk for the file
    /// content.
    pub fn get_blob_batch<F>(&self, keys: Vec<Result<Key>>, local_only: bool, resolve: F)
    where
        F: Fn(usize, Result<Option<Vec<u8>>>) -> (),
    {
        match self {
            Old(stores) => stores.get_blob_batch(keys, local_only, resolve),
            New(stores) => stores.get_blob_batch(keys, local_only, resolve),
        }
    }

    pub fn get_tree(&self, node: &[u8], local_only: bool) -> Result<Option<List>> {
        match self {
            Old(stores) => stores.get_tree(node, local_only),
            New(stores) => stores.get_tree(node, local_only),
        }
    }

    /// Fetch tree contents in batch. Whenever a tree is fetched, the supplied `resolve` function is
    /// called with the tree content or an error message, and the index of the tree in the request
    /// array. When `local_only` is enabled, this function will only check local disk for the file
    /// content.
    pub fn get_tree_batch<F>(&self, keys: Vec<Result<Key>>, local_only: bool, resolve: F)
    where
        F: Fn(usize, Result<Option<List>>) -> (),
    {
        match self {
            Old(stores) => stores.get_tree_batch(keys, local_only, resolve),
            New(stores) => stores.get_tree_batch(keys, local_only, resolve),
        }
    }

    /// Forces backing store to write its pending data to disk and to read the latest version from
    /// the disk.
    pub fn refresh(&self) {
        match self {
            Old(stores) => stores.refresh(),
            New(stores) => stores.refresh(),
        }
    }
}

impl Drop for BackingStore {
    fn drop(&mut self) {
        // Make sure that all the data that was fetched is written to the hgcache.
        self.refresh();
    }
}
