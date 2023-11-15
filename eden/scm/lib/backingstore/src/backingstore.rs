/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use async_runtime::block_on;
use log::warn;
use repo::repo::Repo;
use storemodel::BoxIterator;
use storemodel::Bytes;
use storemodel::FileAuxData;
use storemodel::FileStore;
use storemodel::TreeEntry;
use storemodel::TreeStore;
use tracing::instrument;
use types::HgId;
use types::Key;
use types::RepoPath;

use crate::FetchMode;

pub struct BackingStore {
    filestore: Arc<dyn FileStore>,
    treestore: Arc<dyn TreeStore>,
    repo: Repo,
}

impl BackingStore {
    /// Initialize `BackingStore` with the `allow_retries` setting.
    pub fn new<P: AsRef<Path>>(root: P, allow_retries: bool) -> Result<Self> {
        Self::new_impl(root.as_ref(), allow_retries, &[])
    }

    /// Initialize `BackingStore` with the `allow_retries` setting and extra configs.
    /// This is used by benches/ to set cache path to control warm/code test cases.
    pub fn new_with_config(
        root: impl AsRef<Path>,
        allow_retries: bool,
        extra_configs: &[String],
    ) -> Result<Self> {
        Self::new_impl(root.as_ref(), allow_retries, extra_configs)
    }

    fn new_impl(root: &Path, allow_retries: bool, extra_configs: &[String]) -> Result<Self> {
        constructors::init();

        let root = root.as_ref();
        let mut config = configloader::hg::load(Some(root), &extra_configs, &[])?;

        let source = "backingstore".into();
        config.set("store", "aux", Some("true"), &source);
        if !allow_retries {
            config.set("lfs", "backofftimes", Some(""), &source);
            config.set("lfs", "throttlebackofftimes", Some(""), &source);
            config.set("edenapi", "max-retry-per-request", Some("0"), &source);
        }

        // Apply indexed log configs, which can affect edenfs behavior.
        indexedlog::config::configure(&config)?;

        let mut repo = Repo::load_with_config(root, config.clone())?;
        let filestore = repo.file_store()?;
        let treestore = repo.tree_store()?;

        Ok(Self {
            treestore,
            filestore,
            repo,
        })
    }

    #[instrument(level = "debug", skip(self))]
    pub fn get_blob(&self, node: &[u8], fetch_mode: FetchMode) -> Result<Option<Vec<u8>>> {
        self.filestore.single(node, fetch_mode)
    }

    /// Fetch file contents in batch. Whenever a blob is fetched, the supplied `resolve` function is
    /// called with the file content or an error message, and the index of the blob in the request
    /// array.
    #[instrument(level = "debug", skip(self, resolve))]
    pub fn get_blob_batch<F>(&self, keys: Vec<Key>, fetch_mode: FetchMode, resolve: F)
    where
        F: Fn(usize, Result<Option<Vec<u8>>>),
    {
        self.filestore
            .batch_with_callback(keys, fetch_mode, resolve)
    }

    #[instrument(level = "debug", skip(self))]
    pub fn get_manifest(&mut self, node: &[u8]) -> Result<[u8; 20]> {
        let hgid = HgId::from_slice(node)?;
        let root_tree_id = match block_on(self.repo.get_root_tree_id(hgid)) {
            Ok(root_tree_id) => root_tree_id,
            Err(_e) => {
                // This call may fail with a `NotFoundError` if the revision in question
                // was added to the repository after we originally opened it. Invalidate
                // the repository and try again, in case our cached repo data is just stale.
                self.repo.invalidate_all()?;
                block_on(self.repo.get_root_tree_id(hgid))?
            }
        };
        Ok(root_tree_id.into_byte_array())
    }

    #[instrument(level = "debug", skip(self))]
    pub fn get_tree(
        &self,
        node: &[u8],
        fetch_mode: FetchMode,
    ) -> Result<Option<Box<dyn TreeEntry>>> {
        self.treestore.single(node, fetch_mode)
    }

    /// Fetch tree contents in batch. Whenever a tree is fetched, the supplied `resolve` function is
    /// called with the tree content or an error message, and the index of the tree in the request
    /// array.
    #[instrument(level = "debug", skip(self, resolve))]
    pub fn get_tree_batch<F>(&self, keys: Vec<Key>, fetch_mode: FetchMode, resolve: F)
    where
        F: Fn(usize, Result<Option<Box<dyn TreeEntry>>>),
    {
        self.treestore
            .batch_with_callback(keys, fetch_mode, resolve)
    }

    pub fn get_file_aux(&self, node: &[u8], fetch_mode: FetchMode) -> Result<Option<FileAuxData>> {
        self.filestore.single(node, fetch_mode)
    }

    pub fn get_file_aux_batch<F>(&self, keys: Vec<Key>, fetch_mode: FetchMode, resolve: F)
    where
        F: Fn(usize, Result<Option<FileAuxData>>),
    {
        self.filestore
            .batch_with_callback(keys, fetch_mode, resolve)
    }

    /// Forces backing store to rescan pack files or local indexes
    #[instrument(level = "debug", skip(self))]
    pub fn refresh(&self) {
        self.filestore.refresh().ok();
        self.treestore.refresh().ok();
    }

    #[instrument(level = "debug", skip(self))]
    pub fn flush(&self) {
        self.filestore.flush().ok();
        self.treestore.flush().ok();
    }
}

/// Given a single point local fetch function, and a "streaming" (via iterator)
/// remote fetch function, provide `batch_with_callback` for ease-of-use.
trait LocalRemoteImpl<IntermediateType, OutputType = IntermediateType>
where
    IntermediateType: Into<OutputType>,
{
    fn get_local_single(&self, path: &RepoPath, id: HgId) -> Result<Option<IntermediateType>>;
    fn get_single(&self, path: &RepoPath, id: HgId) -> Result<IntermediateType>;
    fn get_batch_iter(
        &self,
        keys: Vec<Key>,
    ) -> Result<BoxIterator<Result<(Key, IntermediateType)>>>;

    // The following methods are "derived" from the above.

    fn single(&self, node: &[u8], fetch_mode: FetchMode) -> Result<Option<OutputType>> {
        let hgid = HgId::from_slice(node)?;
        if fetch_mode.is_local() {
            let maybe_value = match self.get_local_single(RepoPath::empty(), hgid)? {
                Some(v) => Some(v.into()),
                None => None,
            };
            Ok(maybe_value)
        } else {
            let value = self.get_single(RepoPath::empty(), hgid)?;
            let value = value.into();
            Ok(Some(value))
        }
    }

    fn batch_with_callback<F>(&self, keys: Vec<Key>, fetch_mode: FetchMode, resolve: F)
    where
        F: Fn(usize, Result<Option<OutputType>>),
    {
        if fetch_mode.is_local() {
            // PERF: In some cases this might be sped up using threads in theory.
            // But this needs to be backed by real benchmark data. Besides, edenfs
            // does not call into this path often.
            for (i, key) in keys.iter().enumerate() {
                let result = self.get_local_single(&key.path, key.hgid);
                let result: Result<Option<OutputType>> = match result {
                    Ok(Some(v)) => Ok(Some(v.into())),
                    Ok(None) => Ok(None),
                    Err(e) => Err(e),
                };
                resolve(i, result);
            }
        } else {
            let mut key_to_index = indexed_keys(&keys);
            let mut remaining = keys.len();
            let mut errors = Vec::new();
            match self.get_batch_iter(keys) {
                Err(e) => errors.push(e),
                Ok(iter) => {
                    for entry in iter {
                        let (key, data) = match entry {
                            Err(e) => {
                                errors.push(e);
                                continue;
                            }
                            Ok(v) => v,
                        };
                        if let Some(entry) = key_to_index.get_mut(&key) {
                            if let Some(index) = *entry {
                                *entry = None;
                                remaining = remaining.saturating_sub(1);
                                let result = Ok(Some(data.into()));
                                resolve(index, result);
                            }
                        }
                    }
                }
            }
            if remaining > 0 {
                // Report errors. We don't know the index -> error mapping so
                // we bundle all errors we received.
                let error = ErrorCollection(Arc::new(errors));
                for (_key, entry) in key_to_index.into_iter() {
                    if let Some(index) = entry {
                        resolve(index, Err(error.clone().into()));
                    }
                }
            }
        }
    }
}

/// Read file content.
impl LocalRemoteImpl<Bytes, Vec<u8>> for Arc<dyn FileStore> {
    fn get_local_single(&self, path: &RepoPath, id: HgId) -> Result<Option<Bytes>> {
        self.get_local_content(path, id)
    }
    fn get_single(&self, path: &RepoPath, id: HgId) -> Result<Bytes> {
        self.get_content(path, id)
    }
    fn get_batch_iter(&self, keys: Vec<Key>) -> Result<BoxIterator<Result<(Key, Bytes)>>> {
        self.get_content_iter(keys)
    }
}

/// Read file aux.
impl LocalRemoteImpl<FileAuxData> for Arc<dyn FileStore> {
    fn get_local_single(&self, path: &RepoPath, id: HgId) -> Result<Option<FileAuxData>> {
        self.get_local_aux(path, id)
    }
    fn get_single(&self, path: &RepoPath, id: HgId) -> Result<FileAuxData> {
        self.get_aux(path, id)
    }
    fn get_batch_iter(&self, keys: Vec<Key>) -> Result<BoxIterator<Result<(Key, FileAuxData)>>> {
        self.get_aux_iter(keys)
    }
}

/// Read tree content.
impl LocalRemoteImpl<Box<dyn TreeEntry>> for Arc<dyn TreeStore> {
    fn get_local_single(&self, path: &RepoPath, id: HgId) -> Result<Option<Box<dyn TreeEntry>>> {
        self.get_local_tree(path, id)
    }
    fn get_single(&self, path: &RepoPath, id: HgId) -> Result<Box<dyn TreeEntry>> {
        match self
            .get_tree_iter(vec![Key::new(path.to_owned(), id)])?
            .next()
        {
            Some(Ok((_key, tree))) => Ok(tree),
            Some(Err(e)) => Err(e),
            None => Err(anyhow::format_err!("{}@{}: not found remotely", path, id)),
        }
    }
    fn get_batch_iter(
        &self,
        keys: Vec<Key>,
    ) -> Result<BoxIterator<Result<(Key, Box<dyn TreeEntry>)>>> {
        self.get_tree_iter(keys)
    }
}

/// This type is just for display.
#[derive(Debug, Clone)]
struct ErrorCollection(Arc<Vec<anyhow::Error>>);

impl fmt::Display for ErrorCollection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some((first, rest)) = self.0.split_first() {
            first.fmt(f)?;

            let n = rest.len();
            if n > 0 {
                write!(f, "\n-- and {n} more errors --\n")?;
                for e in rest {
                    e.fmt(f)?;
                    write!(f, "\n--\n")?;
                }
            }
        }
        Ok(())
    }
}

impl std::error::Error for ErrorCollection {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.first().map(|e| e.as_ref())
    }
}

/// Index &[Key] so they can be converted back to the index.
fn indexed_keys(keys: &[Key]) -> HashMap<Key, Option<usize>> {
    keys.iter()
        .cloned()
        .enumerate()
        .map(|(i, k)| (k, Some(i)))
        .collect()
}

impl Drop for BackingStore {
    fn drop(&mut self) {
        // Make sure that all the data that was fetched is written to the hgcache.
        self.flush();
    }
}
