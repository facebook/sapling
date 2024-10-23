/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::Result;
use manifest::testutil::*;
use manifest::Manifest;
use minibytes::Bytes;
use parking_lot::RwLock;
use storemodel::InsertOpts;
use storemodel::KeyStore;
use storemodel::SerializationFormat;
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
#[derive(Default, Clone)]
pub struct TestStore {
    inner: Arc<RwLock<TestStoreInner>>,
}

#[derive(Default)]
pub struct TestStoreInner {
    entries: HashMap<RepoPathBuf, HashMap<HgId, Bytes>>,
    pub prefetched: Vec<Vec<Key>>,
    format: SerializationFormat,
    key_fetch_count: AtomicU64,
    insert_count: AtomicU64,
}

impl TestStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_format(self, format: SerializationFormat) -> Self {
        self.inner.write().format = format;
        self
    }

    #[allow(unused)]
    pub fn fetches(&self) -> Vec<Vec<Key>> {
        self.inner.read().prefetched.clone()
    }

    pub fn key_fetch_count(&self) -> u64 {
        self.inner.read().key_fetch_count.load(Ordering::Relaxed)
    }

    pub fn insert_count(&self) -> u64 {
        self.inner.read().insert_count.load(Ordering::Relaxed)
    }
}

fn compute_sha1(content: &[u8]) -> HgId {
    format_util::hg_sha1_digest(content, HgId::null_id(), HgId::null_id())
}

impl KeyStore for TestStore {
    fn get_local_content(&self, path: &RepoPath, hgid: HgId) -> anyhow::Result<Option<Bytes>> {
        let mut inner = self.inner.write();
        inner.key_fetch_count.fetch_add(1, Ordering::Relaxed);
        let underlying = &mut inner.entries;
        let result = underlying
            .get(path)
            .and_then(|hgid_hash| hgid_hash.get(&hgid))
            .cloned();
        Ok(result)
    }

    fn insert_data(&self, opts: InsertOpts, path: &RepoPath, data: &[u8]) -> anyhow::Result<HgId> {
        let mut inner = self.inner.write();
        inner.insert_count.fetch_add(1, Ordering::Relaxed);
        let underlying = &mut inner.entries;
        let hgid = match opts.forced_id {
            Some(id) => *id,
            None => compute_sha1(data),
        };
        underlying
            .entry(path.to_owned())
            .or_insert(HashMap::new())
            .insert(hgid, Bytes::copy_from_slice(data));
        Ok(hgid)
    }

    fn prefetch(&self, keys: Vec<Key>) -> Result<()> {
        let mut inner = self.inner.write();
        inner
            .key_fetch_count
            .fetch_add(keys.len() as u64, Ordering::Relaxed);
        inner.prefetched.push(keys);
        Ok(())
    }

    fn format(&self) -> SerializationFormat {
        self.inner.read().format
    }

    fn clone_key_store(&self) -> Box<dyn KeyStore> {
        Box::new(self.clone())
    }
}

impl TreeStore for TestStore {
    fn clone_tree_store(&self) -> Box<dyn TreeStore> {
        Box::new(self.clone())
    }
}
