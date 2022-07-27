/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::format_err;
use anyhow::Result;
use manifest::testutil::*;
use manifest::Manifest;
use minibytes::Bytes;
use parking_lot::Mutex;
use parking_lot::RwLock;
use storemodel::TreeFormat;
use types::testutil::*;
use types::HgId;
use types::Key;
use types::RepoPath;
use types::RepoPathBuf;

use crate::FileMetadata;
use crate::TreeManifest;
use crate::TreeStore;

pub fn make_tree_manifest<'a>(
    store: Arc<TestStore>,
    paths: impl IntoIterator<Item = &'a (&'a str, &'a str)>,
) -> TreeManifest {
    let mut tree = TreeManifest::ephemeral(store);
    for (path, filenode) in paths {
        tree.insert(repo_path_buf(path), make_meta(filenode))
            .unwrap();
    }
    tree
}

pub fn make_tree_manifest_from_meta(
    store: Arc<TestStore>,
    paths: impl IntoIterator<Item = (RepoPathBuf, FileMetadata)>,
) -> TreeManifest {
    let mut tree = TreeManifest::ephemeral(store);
    for (path, meta) in paths {
        tree.insert(path, meta).unwrap();
    }
    tree
}

/// An in memory `Store` implementation backed by HashMaps. Primarily intended for tests.
pub struct TestStore {
    entries: RwLock<HashMap<RepoPathBuf, HashMap<HgId, Bytes>>>,
    pub prefetched: Mutex<Vec<Vec<Key>>>,
    format: TreeFormat,
}

impl TestStore {
    pub fn new() -> Self {
        TestStore {
            entries: RwLock::new(HashMap::new()),
            prefetched: Mutex::new(Vec::new()),
            format: TreeFormat::Hg,
        }
    }

    pub fn with_format(mut self, format: TreeFormat) -> Self {
        self.format = format;
        self
    }

    #[allow(unused)]
    pub fn fetches(&self) -> Vec<Vec<Key>> {
        self.prefetched.lock().clone()
    }
}

impl TreeStore for TestStore {
    fn get(&self, path: &RepoPath, hgid: HgId) -> Result<Bytes> {
        let underlying = self.entries.read();
        let result = underlying
            .get(path)
            .and_then(|hgid_hash| hgid_hash.get(&hgid))
            .map(|entry| entry.clone());
        result.ok_or_else(|| format_err!("Could not find manifest entry for ({}, {})", path, hgid))
    }

    fn insert(&self, path: &RepoPath, hgid: HgId, data: Bytes) -> Result<()> {
        let mut underlying = self.entries.write();
        underlying
            .entry(path.to_owned())
            .or_insert(HashMap::new())
            .insert(hgid, data);
        Ok(())
    }

    fn prefetch(&self, keys: Vec<Key>) -> Result<()> {
        self.prefetched.lock().push(keys);
        Ok(())
    }

    fn format(&self) -> TreeFormat {
        self.format
    }
}
