/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use log::warn;
use manifest::List;
use revisionstore::scmstore::file::FileAuxData;
use revisionstore::scmstore::FetchMode;
use revisionstore::scmstore::FileAttributes;
use revisionstore::scmstore::FileStore;
use revisionstore::scmstore::FileStoreBuilder;
use revisionstore::scmstore::KeyFetchError;
use revisionstore::scmstore::StoreFile;
use revisionstore::scmstore::TreeStore;
use revisionstore::scmstore::TreeStoreBuilder;
use revisionstore::HgIdDataStore;
use revisionstore::MemcacheStore;
use tracing::event;
use tracing::instrument;
use tracing::Level;
use types::HgId;
use types::Key;
use types::RepoPathBuf;

pub struct BackingStore {
    filestore: Arc<FileStore>,
    treestore: Arc<TreeStore>,
}

impl BackingStore {
    pub fn new<P: AsRef<Path>>(root: P, allow_retries: bool) -> Result<Self> {
        let root = root.as_ref();
        let mut config = configloader::hg::load(Some(root), &[], &[])?;

        if !allow_retries {
            let source = configloader::config::Options::new().source("backingstore");
            config.set("lfs", "backofftimes", Some(""), &source);
            config.set("lfs", "throttlebackofftimes", Some(""), &source);
            config.set("edenapi", "max-retry-per-request", Some("0"), &source);
        }

        let ident = identity::must_sniff_dir(root)?;
        let hg = root.join(ident.dot_dir());
        let store_path = hg.join("store");

        let mut filestore = FileStoreBuilder::new(&config)
            .local_path(&store_path)
            .store_aux_data();

        let treestore = TreeStoreBuilder::new(&config)
            .local_path(&store_path)
            .suffix(Path::new("manifests"));

        // Memcache takes 30s to initialize on debug builds slowing down tests significantly, let's
        // not even try to initialize it then.
        if !cfg!(debug_assertions) {
            match MemcacheStore::new(&config) {
                Ok(memcache) => {
                    // XXX: Add the memcachestore for the treestore.
                    filestore = filestore.memcache(Arc::new(memcache));
                }
                Err(e) => warn!("couldn't initialize Memcache: {}", e),
            }
        }

        let filestore = Arc::new(filestore.build()?);
        let treestore = treestore.filestore(filestore.clone());

        Ok(Self {
            treestore: Arc::new(treestore.build()?),
            filestore,
        })
    }

    pub fn get_blob(&self, node: &[u8], fetch_mode: FetchMode) -> Result<Option<Vec<u8>>> {
        let hgid = HgId::from_slice(node)?;
        let key = Key::new(RepoPathBuf::new(), hgid);
        self.get_blob_by_key(key, fetch_mode)
    }

    #[instrument(level = "debug", skip(self))]
    fn get_blob_by_key(&self, key: Key, fetch_mode: FetchMode) -> Result<Option<Vec<u8>>> {
        if let FetchMode::LocalOnly = fetch_mode {
            event!(Level::TRACE, "attempting to fetch blob locally");
        }
        let fetch_result = self
            .filestore
            .fetch(std::iter::once(key), FileAttributes::CONTENT, fetch_mode)
            .single();

        Ok(if let Some(mut file) = fetch_result? {
            Some(file.file_content()?.into_vec())
        } else {
            None
        })
    }

    fn get_file_attrs_batch<F>(
        &self,
        keys: Vec<Key>,
        fetch_mode: FetchMode,
        resolve: F,
        attrs: FileAttributes,
    ) where
        F: Fn(usize, Result<Option<StoreFile>>) -> (),
    {
        // Resolve key errors
        let requests = keys.into_iter().enumerate();

        // Crate key-index mapping and fail fast for duplicate keys
        let mut indexes: HashMap<Key, usize> = HashMap::new();
        for (index, key) in requests {
            if let Entry::Vacant(vacant) = indexes.entry(key) {
                vacant.insert(index);
            } else {
                resolve(
                    index,
                    Err(anyhow!(
                        "duplicated keys are not supported by get_file_attrs_batch when using scmstore",
                    )),
                );
            }
        }

        // Handle local-only fetching
        if let FetchMode::LocalOnly = fetch_mode {
            event!(Level::TRACE, "attempting to fetch file aux data locally");
        }

        let fetch_results = self
            .filestore
            .fetch(indexes.keys().cloned(), attrs, fetch_mode);

        for result in fetch_results {
            match result {
                Ok((key, value)) => {
                    if let Some(index) = indexes.remove(&key) {
                        resolve(index, Ok(Some(value)));
                    }
                }
                Err(err) => {
                    match err {
                        KeyFetchError::KeyedError { key, mut errors } => {
                            if let Some(index) = indexes.remove(&key) {
                                if let Some(err) = errors.pop() {
                                    resolve(index, Err(err));
                                } else {
                                    resolve(index, Ok(None));
                                }
                            } else {
                                tracing::error!(
                                    "no index found for {}, scmstore returned a key we have no record of requesting",
                                    key
                                );
                            }
                        }
                        KeyFetchError::Other(_) => {
                            // TODO: How should we handle normal non-keyed errors?
                        }
                    };
                }
            }
        }
    }

    /// Fetch file contents in batch. Whenever a blob is fetched, the supplied `resolve` function is
    /// called with the file content or an error message, and the index of the blob in the request
    /// array.
    #[instrument(level = "debug", skip(self, resolve))]
    pub fn get_blob_batch<F>(&self, keys: Vec<Key>, fetch_mode: FetchMode, resolve: F)
    where
        F: Fn(usize, Result<Option<Vec<u8>>>) -> (),
    {
        self.get_file_attrs_batch(
            keys,
            fetch_mode,
            move |idx, res| {
                resolve(
                    idx,
                    res.transpose()
                        .map(|res| {
                            res.and_then(|mut file| {
                                file.file_content().map(|content| content.into_vec())
                            })
                        })
                        .transpose(),
                )
            },
            FileAttributes::CONTENT,
        )
    }

    #[instrument(level = "debug", skip(self))]
    pub fn get_tree(&self, node: &[u8], fetch_mode: FetchMode) -> Result<Option<List>> {
        let hgid = HgId::from_slice(node)?;
        let key = Key::new(RepoPathBuf::new(), hgid);

        if let FetchMode::LocalOnly = fetch_mode {
            event!(Level::TRACE, "attempting to fetch trees locally");
        }
        let fetch_results = self.treestore.fetch_batch(std::iter::once(key), fetch_mode);

        if let Some(mut entry) = fetch_results.single()? {
            Ok(Some(entry.manifest_tree_entry()?.try_into()?))
        } else {
            Ok(None)
        }
    }

    /// Fetch tree contents in batch. Whenever a tree is fetched, the supplied `resolve` function is
    /// called with the tree content or an error message, and the index of the tree in the request
    /// array.
    #[instrument(level = "debug", skip(self, resolve))]
    pub fn get_tree_batch<F>(&self, keys: Vec<Key>, fetch_mode: FetchMode, resolve: F)
    where
        F: Fn(usize, Result<Option<List>>) -> (),
    {
        // Handle key errors
        let requests = keys.into_iter().enumerate();

        // Crate key-index mapping and fail fast for duplicate keys
        let mut indexes: HashMap<Key, usize> = HashMap::new();
        for (index, key) in requests {
            if let Entry::Vacant(vacant) = indexes.entry(key) {
                vacant.insert(index);
            } else {
                resolve(
                    index,
                    Err(anyhow!(
                        "duplicated keys are not supported by get_tree_batch when using scmstore",
                    )),
                );
            }
        }

        // Handle local-only fetching
        if let FetchMode::LocalOnly = fetch_mode {
            event!(Level::TRACE, "attempting to fetch trees locally");
        }
        let fetch_results = self
            .treestore
            .fetch_batch(indexes.keys().cloned(), fetch_mode);

        // Handle pey-key fetch results
        for result in fetch_results {
            match result {
                Ok((key, mut value)) => {
                    if let Some(index) = indexes.remove(&key) {
                        resolve(
                            index,
                            Some(value.manifest_tree_entry().and_then(|t| t.try_into()))
                                .transpose(),
                        );
                    }
                }
                Err(err) => {
                    match err {
                        KeyFetchError::KeyedError { key, mut errors } => {
                            if let Some(index) = indexes.remove(&key) {
                                if let Some(err) = errors.pop() {
                                    resolve(index, Err(err));
                                } else {
                                    resolve(index, Ok(None));
                                }
                            } else {
                                tracing::error!(
                                    "no index found for {}, scmstore returned a key we have no record of requesting",
                                    key
                                );
                            }
                        }
                        KeyFetchError::Other(_) => {
                            // TODO: How should we handle normal non-keyed errors?
                        }
                    };
                }
            }
        }
    }

    pub fn get_file_aux(&self, node: &[u8], fetch_mode: FetchMode) -> Result<Option<FileAuxData>> {
        let hgid = HgId::from_slice(node)?;
        let key = Key::new(RepoPathBuf::new(), hgid);

        if let FetchMode::LocalOnly = fetch_mode {
            event!(Level::TRACE, "attempting to fetch file aux data locally");
        }
        let fetch_results =
            self.filestore
                .fetch(std::iter::once(key), FileAttributes::AUX, fetch_mode);

        if let Some(entry) = fetch_results.single()? {
            Ok(Some(entry.aux_data()?.try_into()?))
        } else {
            Ok(None)
        }
    }

    pub fn get_file_aux_batch<F>(&self, keys: Vec<Key>, fetch_mode: FetchMode, resolve: F)
    where
        F: Fn(usize, Result<Option<FileAuxData>>) -> (),
    {
        self.get_file_attrs_batch(
            keys,
            fetch_mode,
            move |idx, res| {
                resolve(
                    idx,
                    res.transpose()
                        .map(|res| res.and_then(|file| file.aux_data()))
                        .transpose(),
                )
            },
            FileAttributes::AUX,
        )
    }

    /// Forces backing store to rescan pack files or local indexes
    #[instrument(level = "debug", skip(self))]
    pub fn flush(&self) {
        self.filestore.refresh().ok();
        self.treestore.refresh().ok();
    }
}

impl Drop for BackingStore {
    fn drop(&mut self) {
        // Make sure that all the data that was fetched is written to the hgcache.
        self.flush();
    }
}
