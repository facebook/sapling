/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use blob::Blob;
use manifest::Manifest;
use manifest::testutil::*;
use minibytes::Bytes;
use parking_lot::RwLock;
use storemodel::BoxIterator;
use storemodel::BoxRefIterator;
use storemodel::ContentFetchItems;
use storemodel::FileStore;
use storemodel::InsertOpts;
use storemodel::KeyStore;
use storemodel::Kind;
use storemodel::SerializationFormat;
use storemodel::TreeEntry;
use storemodel::TreeFetchItems;
use types::FetchContext;
use types::HgId;
use types::Key;
use types::PathComponent;
use types::PathComponentBuf;
use types::RepoPath;
use types::RepoPathBuf;
use types::testutil::*;
use types::tree::TreeItemFlag;

use crate::FileMetadata;
use crate::TreeManifest;
use crate::TreeStore;
use crate::link::LinkData::*;

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
    entries: HashMap<HgId, Bytes>,
    // Calls to get_content_iter() and get_tree_iter().
    fetched: Vec<Vec<Key>>,
    // FetchContexts passed to get_content_iter().
    fetch_contexts: Vec<FetchContext>,
    // Parents recorded via insert_data with InsertOpts.parents.
    parents: HashMap<(RepoPathBuf, HgId), Vec<HgId>>,
    // ACL children indices recorded via insert_data with InsertOpts.acl_children_indices.
    acl_children_indices: HashMap<HgId, Vec<u32>>,
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
        self.inner.read().fetched.clone()
    }

    #[allow(unused)]
    pub fn fetch_contexts(&self) -> Vec<FetchContext> {
        self.inner.read().fetch_contexts.clone()
    }

    pub fn key_fetch_count(&self) -> u64 {
        self.inner.read().key_fetch_count.load(Ordering::Relaxed)
    }

    pub fn insert_count(&self) -> u64 {
        self.inner.read().insert_count.load(Ordering::Relaxed)
    }

    pub fn get_parents(&self, path: &RepoPath, hgid: HgId) -> Option<Vec<HgId>> {
        self.inner
            .read()
            .parents
            .get(&(path.to_owned(), hgid))
            .cloned()
    }

    pub fn set_acl_children_indices(&self, hgid: HgId, indices: Vec<u32>) {
        self.inner
            .write()
            .acl_children_indices
            .insert(hgid, indices);
    }

    pub fn get_acl_children_indices(&self, hgid: HgId) -> Option<Vec<u32>> {
        self.inner.read().acl_children_indices.get(&hgid).cloned()
    }
}

struct TestTreeEntry {
    entry: crate::store::Entry,
    acl_indices: Option<Vec<u32>>,
}

impl TreeEntry for TestTreeEntry {
    fn iter<'a>(
        &'a self,
    ) -> anyhow::Result<BoxRefIterator<'a, anyhow::Result<(&'a PathComponent, HgId, TreeItemFlag)>>>
    {
        self.entry.iter()
    }

    fn iter_owned(
        &self,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(PathComponentBuf, HgId, TreeItemFlag)>>> {
        self.entry.iter_owned()
    }

    fn lookup(&self, name: &PathComponent) -> anyhow::Result<Option<(HgId, TreeItemFlag)>> {
        self.entry.lookup(name)
    }

    fn children_with_acls(&self) -> anyhow::Result<Vec<(PathComponentBuf, HgId)>> {
        let indices = match &self.acl_indices {
            Some(indices) if !indices.is_empty() => indices,
            _ => return Ok(Vec::new()),
        };
        let index_set: HashSet<u32> = indices.iter().copied().collect();
        let mut result = Vec::with_capacity(indices.len());
        for (idx, elem) in self.entry.iter_owned()?.enumerate() {
            let (path, hgid, flag) = elem?;
            if index_set.contains(&(idx as u32)) && matches!(flag, TreeItemFlag::Directory) {
                result.push((path, hgid));
            }
        }
        Ok(result)
    }

    fn size_hint(&self) -> Option<usize> {
        self.entry.size_hint()
    }
}

impl KeyStore for TestStore {
    fn get_content_iter(
        &self,
        fctx: FetchContext,
        keys: Vec<Key>,
    ) -> anyhow::Result<ContentFetchItems> {
        let mut inner = self.inner.write();
        inner
            .key_fetch_count
            .fetch_add(keys.len() as u64, Ordering::Relaxed);
        inner.fetched.push(keys.clone());
        inner.fetch_contexts.push(fctx);
        let entries = inner.entries.clone();
        drop(inner);
        let iter = keys
            .into_iter()
            .map(move |k| match entries.get(&k.hgid).cloned() {
                Some(data) => Ok((k, Blob::Bytes(data))),
                None => Err(anyhow::format_err!(
                    "{}@{}: not found locally",
                    k.path,
                    k.hgid
                )),
            });
        Ok(ContentFetchItems::item_stream(iter))
    }

    fn get_local_content(&self, _path: &RepoPath, hgid: HgId) -> anyhow::Result<Option<Blob>> {
        let inner = self.inner.read();
        let result = inner.entries.get(&hgid).cloned();
        Ok(result.map(Blob::Bytes))
    }

    fn insert_data(&self, opts: InsertOpts, path: &RepoPath, data: Blob) -> anyhow::Result<HgId> {
        let mut inner = self.inner.write();
        inner.insert_count.fetch_add(1, Ordering::Relaxed);
        let format = inner.format;
        let data_bytes = data.to_bytes();
        let hgid = match opts.forced_id {
            Some(id) => *id,
            None => match format {
                SerializationFormat::Hg => {
                    let p1 = opts.parents.first().unwrap_or(HgId::null_id());
                    let p2 = opts.parents.get(1).unwrap_or(HgId::null_id());
                    format_util::hg_sha1_digest(&data_bytes, p1, p2)
                }
                SerializationFormat::Git => {
                    let kind = match opts.kind {
                        Kind::Tree => "tree",
                        Kind::File => "blob",
                    };
                    format_util::git_sha1_digest(&data_bytes, kind)
                }
            },
        };
        inner.entries.insert(hgid, data_bytes);
        if !opts.parents.is_empty() {
            inner.parents.insert((path.to_owned(), hgid), opts.parents);
        }
        if let Some(indices) = opts.acl_children_indices {
            if !indices.is_empty() {
                inner.acl_children_indices.insert(hgid, indices);
            }
        }
        Ok(hgid)
    }

    fn format(&self) -> SerializationFormat {
        self.inner.read().format
    }

    fn clone_key_store(&self) -> Box<dyn KeyStore> {
        Box::new(self.clone())
    }
}

impl TreeStore for TestStore {
    fn get_tree_iter(&self, _fctx: FetchContext, keys: Vec<Key>) -> anyhow::Result<TreeFetchItems> {
        let mut inner = self.inner.write();
        inner
            .key_fetch_count
            .fetch_add(keys.len() as u64, Ordering::Relaxed);
        inner.fetched.push(keys.clone());
        drop(inner);

        let store = self.clone_tree_store();
        let iter = keys
            .into_iter()
            .map(move |k| match store.get_local_tree(&k.path, k.hgid) {
                Err(e) => Err(e),
                Ok(None) => Err(anyhow::format_err!(
                    "{}@{}: not found locally",
                    k.path,
                    k.hgid
                )),
                Ok(Some(data)) => Ok((k, data)),
            });
        Ok(TreeFetchItems::item_stream(iter))
    }

    fn get_local_tree(
        &self,
        _path: &RepoPath,
        hgid: HgId,
    ) -> anyhow::Result<Option<Arc<dyn TreeEntry>>> {
        let inner = self.inner.read();
        match inner.entries.get(&hgid) {
            Some(data) => {
                let format = inner.format;
                let entry = crate::store::Entry(data.clone(), format);
                let acl_indices = inner.acl_children_indices.get(&hgid).cloned();
                Ok(Some(Arc::new(TestTreeEntry { entry, acl_indices })))
            }
            None => Ok(None),
        }
    }

    fn clone_tree_store(&self) -> Box<dyn TreeStore> {
        Box::new(self.clone())
    }
}

impl FileStore for TestStore {
    fn clone_file_store(&self) -> Box<dyn FileStore + 'static> {
        Box::new(self.clone())
    }
}

/// Get the hgid for a path in a TreeManifest. Works for both files and directories.
/// Panics if the path is not found or is ephemeral.
pub fn get_hgid(tree: &TreeManifest, path: &RepoPath) -> HgId {
    match tree.get_link(path).unwrap().unwrap().as_ref() {
        Leaf(file_metadata) => file_metadata.hgid,
        Durable(entry) => entry.hgid,
        Ephemeral(_) => {
            panic!("Asked for hgid on path {path} but found ephemeral hgid.")
        }
    }
}
