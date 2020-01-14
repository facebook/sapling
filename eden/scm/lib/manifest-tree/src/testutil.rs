/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{collections::HashMap, sync::Arc};

use anyhow::{format_err, Result};
use bytes::Bytes;
use parking_lot::{Mutex, RwLock};

use manifest::{testutil::*, Manifest};
use types::{testutil::*, HgId, Key, RepoPath, RepoPathBuf};

use crate::{TreeManifest, TreeStore};

pub fn make_tree_manifest<'a>(
    paths: impl IntoIterator<Item = &'a (&'a str, &'a str)>,
) -> TreeManifest {
    let mut tree = TreeManifest::ephemeral(Arc::new(TestStore::new()));
    for (path, filenode) in paths {
        tree.insert(repo_path_buf(path), make_meta(filenode))
            .unwrap();
    }
    tree
}

/// An in memory `Store` implementation backed by HashMaps. Primarily intended for tests.
pub struct TestStore {
    entries: RwLock<HashMap<RepoPathBuf, HashMap<HgId, Bytes>>>,
    pub prefetched: Mutex<Vec<Vec<Key>>>,
}

impl TestStore {
    pub fn new() -> Self {
        TestStore {
            entries: RwLock::new(HashMap::new()),
            prefetched: Mutex::new(Vec::new()),
        }
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
}
