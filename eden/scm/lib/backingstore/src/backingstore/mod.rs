/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod scmstores;

use std::path::Path;

use anyhow::Result;
use manifest::List;
use revisionstore::scmstore::file::FileAuxData;
use types::Key;

use crate::backingstore::scmstores::BackingScmStores;

pub enum BackingStore {
    New(BackingScmStores),
}

use BackingStore::*;

impl BackingStore {
    pub fn new<P: AsRef<Path>>(root: P, aux_data: bool, allow_retries: bool) -> Result<Self> {
        let root = root.as_ref();
        let mut config = configparser::hg::load(Some(root), &[], &[])?;

        if !allow_retries {
            let source = configparser::config::Options::new().source("backingstore");
            config.set("lfs", "backofftimes", Some(""), &source);
            config.set("lfs", "throttlebackofftimes", Some(""), &source);
            config.set("edenapi", "max-retry-per-request", Some("0"), &source);
        }

        let ident = identity::must_sniff_dir(root)?;
        let dot_path = root.join(ident.dot_dir());

        Ok(New(BackingScmStores::new(&config, &dot_path, aux_data)?))
    }

    /// Reads file from blobstores. When `local_only` is true, this function will only read blobs
    /// from on disk stores.
    pub fn get_blob(&self, path: &[u8], node: &[u8], local_only: bool) -> Result<Option<Vec<u8>>> {
        match self {
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
            New(stores) => stores.get_blob_batch(keys, local_only, resolve),
        }
    }

    pub fn get_tree(&self, node: &[u8], local_only: bool) -> Result<Option<List>> {
        match self {
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
            New(stores) => stores.get_tree_batch(keys, local_only, resolve),
        }
    }

    pub fn get_file_aux(&self, node: &[u8], local_only: bool) -> Result<Option<FileAuxData>> {
        match self {
            New(stores) => stores.get_file_aux(node, local_only),
        }
    }

    pub fn get_file_aux_batch<F>(&self, keys: Vec<Result<Key>>, local_only: bool, resolve: F)
    where
        F: Fn(usize, Result<Option<FileAuxData>>) -> (),
    {
        match self {
            New(stores) => stores.get_file_aux_batch(keys, local_only, resolve),
        }
    }

    /// Forces backing store to write its pending data to disk and to read the latest version from
    /// the disk.
    pub fn flush(&self) {
        match self {
            New(stores) => stores.flush(),
        }
    }
}

impl Drop for BackingStore {
    fn drop(&mut self) {
        // Make sure that all the data that was fetched is written to the hgcache.
        self.flush();
    }
}
