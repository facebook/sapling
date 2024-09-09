/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Borrow;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use ::types::fetch_mode::FetchMode;
use ::types::hgid::NULL_ID;
use ::types::tree::TreeItemFlag;
use ::types::HgId;
use ::types::Key;
use ::types::Node;
use ::types::NodeInfo;
use ::types::Parents;
use ::types::PathComponent;
use ::types::PathComponentBuf;
use ::types::RepoPath;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use cas_client::CasClient;
use clientinfo::get_client_request_info_thread_local;
use clientinfo::set_client_request_info_thread_local;
use crossbeam::channel::unbounded;
use edenapi_types::FileAuxData;
use edenapi_types::TreeAuxData;
use edenapi_types::TreeChildEntry;
use fetch::FetchState;
use minibytes::Bytes;
use once_cell::sync::OnceCell;
use parking_lot::RwLock;
use storemodel::BoxIterator;
use storemodel::SerializationFormat;
use storemodel::TreeEntry;

pub use self::metrics::TreeStoreMetrics;
use crate::datastore::HgIdDataStore;
use crate::datastore::RemoteDataStore;
use crate::indexedlogdatastore::Entry;
use crate::indexedlogdatastore::IndexedLogHgIdDataStore;
use crate::indexedlogtreeauxstore::TreeAuxStore;
use crate::scmstore::fetch::FetchResults;
use crate::scmstore::fetch::KeyFetchError;
use crate::scmstore::file::FileStore;
use crate::scmstore::metrics::StoreLocation;
use crate::scmstore::tree::types::LazyTree;
use crate::scmstore::tree::types::StoreTree;
use crate::scmstore::tree::types::TreeAttributes;
use crate::ContentDataStore;
use crate::ContentMetadata;
use crate::Delta;
use crate::HgIdHistoryStore;
use crate::HgIdMutableDeltaStore;
use crate::HgIdMutableHistoryStore;
use crate::IndexedLogHgIdHistoryStore;
use crate::LegacyStore;
use crate::LocalStore;
use crate::Metadata;
use crate::SaplingRemoteApiTreeStore;
use crate::StoreKey;
use crate::StoreResult;

mod fetch;
mod metrics;
pub mod types;

#[derive(Clone, Debug)]
pub enum TreeMetadataMode {
    Never,
    Always,
    OptIn,
}

#[derive(Clone)]
pub struct TreeStore {
    /// The "local" indexedlog store. Stores content that is created locally.
    pub indexedlog_local: Option<Arc<IndexedLogHgIdDataStore>>,

    /// The "cache" indexedlog store (previously called "shared"). Stores content downloaded from
    /// a remote store.
    pub indexedlog_cache: Option<Arc<IndexedLogHgIdDataStore>>,

    /// If cache_to_local_cache is true, data found by falling back to a remote store
    /// will the written to indexedlog_cache.
    pub cache_to_local_cache: bool,

    /// An SaplingRemoteApi Client, SaplingRemoteApiTreeStore provides the tree-specific subset of SaplingRemoteApi functionality
    /// used by TreeStore.
    pub edenapi: Option<Arc<SaplingRemoteApiTreeStore>>,

    /// A FileStore, which can be used for fetching and caching file aux data for a tree.
    pub filestore: Option<Arc<FileStore>>,

    /// A TreeAuxStore, for storing directory metadata about each tree.
    pub tree_aux_store: Option<Arc<TreeAuxStore>>,

    /// Whether we should request extra children metadata from SaplingRemoteAPI and write back to
    /// filestore's aux cache.
    pub tree_metadata_mode: TreeMetadataMode,

    pub historystore_local: Option<Arc<IndexedLogHgIdHistoryStore>>,
    pub historystore_cache: Option<Arc<IndexedLogHgIdHistoryStore>>,

    pub cas_client: Option<Arc<dyn CasClient>>,

    /// Write tree parents to history cache even if parents weren't requested.
    pub prefetch_tree_parents: bool,

    pub flush_on_drop: bool,

    /// Whether to fetch trees aux data from remote (provided by the augmented trees)
    pub fetch_tree_aux_data: bool,

    pub(crate) metrics: Arc<RwLock<TreeStoreMetrics>>,
}

impl Drop for TreeStore {
    fn drop(&mut self) {
        if self.flush_on_drop {
            let _ = TreeStore::flush(self);
        }
    }
}

impl TreeStore {
    pub fn fetch_batch(
        &self,
        reqs: impl Iterator<Item = Key>,
        attrs: TreeAttributes,
        fetch_mode: FetchMode,
    ) -> FetchResults<StoreTree> {
        let (found_tx, found_rx) = unbounded();
        let found_tx2 = found_tx.clone();
        let mut state = FetchState::new(reqs, attrs, found_tx, fetch_mode);

        let keys_len = state.common.pending_len();

        let indexedlog_cache = self.indexedlog_cache.clone();
        let indexedlog_local = self.indexedlog_local.clone();
        let edenapi = self.edenapi.clone();

        let historystore_cache = self.historystore_cache.clone();
        let historystore_local = self.historystore_local.clone();

        let cache_to_local_cache = self.cache_to_local_cache;
        let aux_cache = self.filestore.as_ref().and_then(|fs| fs.aux_cache.clone());
        let tree_aux_store = self.tree_aux_store.clone();
        let cas_client = self.cas_client.clone();

        let fetch_children_metadata = match self.tree_metadata_mode {
            TreeMetadataMode::Always => true,
            TreeMetadataMode::Never => false,
            TreeMetadataMode::OptIn => fetch_mode.contains(FetchMode::PREFETCH),
        };
        let fetch_tree_aux_data = self.fetch_tree_aux_data || attrs.aux_data;
        let fetch_parents = attrs.parents || self.prefetch_tree_parents;

        let fetch_local = fetch_mode.contains(FetchMode::LOCAL);
        let fetch_remote = fetch_mode.contains(FetchMode::REMOTE);

        tracing::debug!(
            ?fetch_mode,
            ?attrs,
            fetch_children_metadata,
            fetch_tree_aux_data,
            fetch_local,
            fetch_remote,
            keys_len
        );

        let store_metrics = self.metrics.clone();

        let process_func = move || -> Result<()> {
            if fetch_local {
                for (log, location) in [
                    (&indexedlog_cache, StoreLocation::Cache),
                    (&indexedlog_local, StoreLocation::Local),
                ] {
                    if let Some(log) = log {
                        let start_time = Instant::now();

                        let pending: Vec<_> = state
                            .common
                            .pending(TreeAttributes::CONTENT, false)
                            .map(|(key, _attrs)| key.clone())
                            .collect();

                        let store_metrics = state.metrics.indexedlog.store(location);
                        let fetch_count = pending.len();

                        store_metrics.fetch(fetch_count);

                        let mut found_count: usize = 0;
                        for key in pending.into_iter() {
                            if let Some(entry) = log.get_entry(key)? {
                                tracing::trace!("{:?} found in {:?}", entry.key(), location);
                                state
                                    .common
                                    .found(entry.key().clone(), LazyTree::IndexedLog(entry).into());
                                found_count += 1;
                            }
                        }

                        store_metrics.hit(found_count);
                        store_metrics.miss(fetch_count - found_count);
                        let _ = store_metrics.time_from_duration(start_time.elapsed());
                    }
                }

                for (name, log) in [
                    ("cache", &historystore_cache),
                    ("local", &historystore_local),
                ] {
                    if let Some(log) = log {
                        let pending: Vec<_> = state
                            .common
                            .pending(TreeAttributes::PARENTS, false)
                            .map(|(key, _attrs)| key.clone())
                            .collect();
                        for key in pending.into_iter() {
                            if let Some(entry) = log.get_node_info(&key)? {
                                tracing::trace!("{:?} found parents in {name}", key);
                                state.common.found(
                                    key,
                                    StoreTree {
                                        content: None,
                                        parents: Some(Parents::new(
                                            entry.parents[0].hgid,
                                            entry.parents[1].hgid,
                                        )),
                                        aux_data: None,
                                    },
                                );
                            }
                        }
                    }
                }
            }

            if fetch_local || (fetch_remote && cas_client.is_some()) {
                if let Some(tree_aux_store) = &tree_aux_store {
                    let mut wants_aux = TreeAttributes::AUX_DATA;
                    if cas_client.is_some() {
                        wants_aux |= TreeAttributes::CONTENT;
                    }
                    let pending: Vec<_> = state
                        .common
                        .pending(wants_aux, false)
                        .map(|(key, _attrs)| key.clone())
                        .collect();
                    for key in pending.into_iter() {
                        if let Some(entry) = tree_aux_store.get(&key.hgid)? {
                            tracing::trace!(?key, ?entry, "found tree aux entry in cache");
                            if cas_client.is_some() {
                                tracing::trace!(target: "cas", ?key, ?entry, "found tree aux data");
                            }
                            state.common.found(
                                key.clone(),
                                StoreTree {
                                    content: None,
                                    parents: None,
                                    aux_data: Some(entry),
                                },
                            );
                        }
                    }
                }
            }

            if fetch_remote {
                if let Some(cas_client) = &cas_client {
                    state.fetch_cas(cas_client, aux_cache.as_deref(), tree_aux_store.as_deref());
                }

                if let Some(edenapi) = &edenapi {
                    let attributes = edenapi_types::TreeAttributes {
                        manifest_blob: true,
                        // We use parents to check hash integrity.
                        parents: true,
                        // Include file and tree aux data for entries, if available (tree aux data requires augmented_trees=true).
                        child_metadata: fetch_children_metadata,
                        // Use pre-derived "augmented" tree data, which includes tree aux data.
                        augmented_trees: fetch_tree_aux_data,
                    };

                    state.fetch_edenapi(
                        edenapi,
                        attributes,
                        if cache_to_local_cache {
                            indexedlog_cache.as_deref()
                        } else {
                            None
                        },
                        aux_cache.as_deref(),
                        tree_aux_store.as_deref(),
                        if fetch_parents {
                            historystore_cache.as_deref()
                        } else {
                            None
                        },
                    )?;
                } else {
                    tracing::debug!("no SaplingRemoteApi associated with TreeStore");
                }
            }

            // TODO(meyer): Report incomplete / not found, handle errors better instead of just always failing the batch, etc
            state.common.results(state.errors);

            if let Err(err) = state.metrics.update_ods() {
                tracing::error!(?err, "error updating tree ods counters");
            }

            store_metrics.write().fetch += state.metrics;

            Ok(())
        };
        let process_func_errors = move || {
            if let Err(err) = process_func() {
                let _ = found_tx2.send(Err(KeyFetchError::Other(err)));
            }
        };

        // Only kick off a thread if there's a substantial amount of work.
        if keys_len > 1000 {
            let cri = get_client_request_info_thread_local();
            std::thread::spawn(move || {
                if let Some(cri) = cri {
                    set_client_request_info_thread_local(cri);
                }
                process_func_errors();
            });
        } else {
            process_func_errors();
        }

        FetchResults::new(Box::new(found_rx.into_iter()))
    }

    fn write_batch(&self, entries: impl Iterator<Item = (Key, Bytes, Metadata)>) -> Result<()> {
        if let Some(ref indexedlog_local) = self.indexedlog_local {
            for (key, bytes, meta) in entries {
                indexedlog_local.put_entry(Entry::new(key, bytes, meta))?;
            }
        }
        Ok(())
    }

    pub fn empty() -> Self {
        TreeStore {
            indexedlog_local: None,
            indexedlog_cache: None,
            cache_to_local_cache: true,
            edenapi: None,
            cas_client: None,
            historystore_cache: None,
            historystore_local: None,
            filestore: None,
            tree_aux_store: None,
            flush_on_drop: true,
            tree_metadata_mode: TreeMetadataMode::Never,
            fetch_tree_aux_data: false,
            metrics: Default::default(),
            prefetch_tree_parents: false,
        }
    }

    #[allow(unused_must_use)]
    pub fn flush(&self) -> Result<()> {
        let mut result = Ok(());
        let mut handle_error = |error| {
            tracing::error!(%error);
            result = Err(error);
        };

        if let Some(ref indexedlog_local) = self.indexedlog_local {
            indexedlog_local.flush_log().map_err(&mut handle_error);
        }

        if let Some(ref indexedlog_cache) = self.indexedlog_cache {
            indexedlog_cache.flush_log().map_err(&mut handle_error);
        }

        if let Some(ref tree_aux_store) = self.tree_aux_store {
            tree_aux_store.flush().map_err(&mut handle_error);
        }

        if let Some(ref historystore_local) = self.historystore_local {
            historystore_local.flush().map_err(&mut handle_error);
        }

        if let Some(ref historystore_cache) = self.historystore_cache {
            historystore_cache.flush().map_err(&mut handle_error);
        }

        let mut metrics = self.metrics.write();
        for (k, v) in metrics.metrics() {
            hg_metrics::increment_counter(k, v as u64);
        }
        *metrics = Default::default();

        result
    }

    pub fn refresh(&self) -> Result<()> {
        self.flush()
    }
}

impl LegacyStore for TreeStore {
    /// Returns only the local cache / shared stores, in place of the local-only stores, such that writes will go directly to the local cache.
    /// For compatibility with ContentStore::get_shared_mutable
    fn get_shared_mutable(&self) -> Arc<dyn HgIdMutableDeltaStore> {
        // this is infallible in ContentStore so panic if there are no shared/cache stores.
        assert!(
            self.indexedlog_cache.is_some(),
            "cannot get shared_mutable, no shared / local cache stores available"
        );
        Arc::new(TreeStore {
            indexedlog_local: self.indexedlog_cache.clone(),
            indexedlog_cache: None,
            historystore_local: self.historystore_cache.clone(),
            historystore_cache: None,
            cache_to_local_cache: false,
            edenapi: None,
            cas_client: None,
            filestore: None,
            tree_aux_store: None,
            flush_on_drop: true,
            tree_metadata_mode: TreeMetadataMode::Never,
            fetch_tree_aux_data: false,
            metrics: self.metrics.clone(),
            prefetch_tree_parents: false,
        })
    }

    fn get_file_content(&self, _key: &Key) -> Result<Option<Bytes>> {
        unimplemented!(
            "get_file_content is not implemented for trees, it should only ever be falled for files"
        );
    }
}

impl HgIdDataStore for TreeStore {
    // Fetch the raw content of a single TreeManifest blob
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        Ok(
            match self
                .fetch_batch(std::iter::once(key.clone()).filter_map(StoreKey::maybe_into_key), TreeAttributes::CONTENT, FetchMode::AllowRemote)
                .single()?
            {
                Some(entry) => StoreResult::Found(entry.content.expect("content attribute not found despite being requested and returned as complete").hg_content()?.into_vec()),
                None => StoreResult::NotFound(key),
            },
        )
    }

    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        Ok(
            match self
                .fetch_batch(
                    std::iter::once(key.clone()).filter_map(StoreKey::maybe_into_key),
                    TreeAttributes::CONTENT,
                    FetchMode::AllowRemote,
                )
                .single()?
            {
                // This is currently in a bit of an awkward state, as revisionstore metadata is no longer used for trees
                // (it should always be default), but the get_meta function should return StoreResult::Found
                // only when the content is available. Thus, we request the tree content, but ignore it and just
                // return default metadata when it's found, and otherwise report StoreResult::NotFound.
                // TODO(meyer): Replace this with an presence check once support for separate fetch and return attrs
                // is added.
                Some(_e) => StoreResult::Found(Metadata::default()),
                None => StoreResult::NotFound(key),
            },
        )
    }

    fn refresh(&self) -> Result<()> {
        self.refresh()
    }
}

impl RemoteDataStore for TreeStore {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        Ok(self
            .fetch_batch(
                keys.iter().cloned().filter_map(StoreKey::maybe_into_key),
                TreeAttributes::CONTENT,
                FetchMode::AllowRemote,
            )
            .missing()?
            .into_iter()
            .map(StoreKey::HgId)
            .collect())
    }

    fn upload(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        Ok(keys.to_vec())
    }
}

impl LocalStore for TreeStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        let mut missing: Vec<_> = keys.to_vec();

        missing = if let Some(ref indexedlog_cache) = self.indexedlog_cache {
            missing
                .into_iter()
                .filter(|sk| {
                    match sk
                        .maybe_as_key()
                        .map(|k| IndexedLogHgIdDataStore::contains(indexedlog_cache, &k.hgid))
                    {
                        Some(Ok(contains)) => !contains,
                        None | Some(Err(_)) => true,
                    }
                })
                .collect()
        } else {
            missing
        };

        missing = if let Some(ref indexedlog_local) = self.indexedlog_local {
            missing
                .into_iter()
                .filter(|sk| {
                    match sk
                        .maybe_as_key()
                        .map(|k| IndexedLogHgIdDataStore::contains(indexedlog_local, &k.hgid))
                    {
                        Some(Ok(contains)) => !contains,
                        None | Some(Err(_)) => true,
                    }
                })
                .collect()
        } else {
            missing
        };

        Ok(missing)
    }
}

impl HgIdMutableDeltaStore for TreeStore {
    fn add(&self, delta: &Delta, metadata: &Metadata) -> Result<()> {
        if let Delta {
            data,
            base: None,
            key,
        } = delta.clone()
        {
            self.write_batch(std::iter::once((key, data, metadata.clone())))
        } else {
            bail!("Deltas with non-None base are not supported")
        }
    }

    fn flush(&self) -> Result<Option<Vec<PathBuf>>> {
        if let Some(ref indexedlog_local) = self.indexedlog_local {
            indexedlog_local.flush_log()?;
        }
        if let Some(ref indexedlog_cache) = self.indexedlog_cache {
            indexedlog_cache.flush_log()?;
        }
        if let Some(ref tree_aux_store) = self.tree_aux_store {
            tree_aux_store.flush()?;
        }
        Ok(None)
    }
}

impl HgIdHistoryStore for TreeStore {
    fn get_node_info(&self, key: &Key) -> Result<Option<NodeInfo>> {
        self.fetch_batch(
            std::iter::once(key.clone()),
            TreeAttributes::PARENTS,
            FetchMode::AllowRemote,
        )
        .single()
        .map(|t| {
            t.and_then(|t| {
                t.parents.map(|p| NodeInfo {
                    parents: p.to_keys(),
                    linknode: NULL_ID,
                })
            })
        })
    }

    fn refresh(&self) -> Result<()> {
        self.refresh()
    }
}

impl HgIdMutableHistoryStore for TreeStore {
    fn add(&self, key: &Key, info: &NodeInfo) -> Result<()> {
        if let Some(historystore_local) = &self.historystore_local {
            historystore_local.add(key, info)
        } else {
            bail!("no local history store configured");
        }
    }

    fn flush(&self) -> Result<Option<Vec<PathBuf>>> {
        self.flush()?;
        Ok(None)
    }
}

// TODO(meyer): Content addressing not supported at all for trees. I could look for HgIds present here and fetch with
// that if available, but I feel like there's probably something wrong if this is called for trees. Do we need to implement
// this at all for trees?
impl ContentDataStore for TreeStore {
    fn blob(&self, key: StoreKey) -> Result<StoreResult<Bytes>> {
        Ok(StoreResult::NotFound(key))
    }

    fn metadata(&self, key: StoreKey) -> Result<StoreResult<ContentMetadata>> {
        Ok(StoreResult::NotFound(key))
    }
}

impl storemodel::KeyStore for TreeStore {
    fn get_local_content(
        &self,
        path: &RepoPath,
        node: HgId,
    ) -> anyhow::Result<Option<minibytes::Bytes>> {
        if node.is_null() {
            return Ok(Some(Default::default()));
        }
        let key = Key::new(path.to_owned(), node);
        match self
            .fetch_batch(
                std::iter::once(key.clone()),
                TreeAttributes::CONTENT,
                FetchMode::LocalOnly,
            )
            .single()?
        {
            Some(entry) => Ok(Some(entry.content.expect("no tree content").hg_content()?)),
            None => Ok(None),
        }
    }

    fn get_content(
        &self,
        path: &RepoPath,
        node: Node,
        fetch_mode: FetchMode,
    ) -> Result<minibytes::Bytes> {
        if node.is_null() {
            return Ok(Default::default());
        }
        let key = Key::new(path.to_owned(), node);
        match self
            .fetch_batch(
                std::iter::once(key.clone()),
                TreeAttributes::CONTENT,
                fetch_mode,
            )
            .single()?
        {
            Some(entry) => Ok(entry.content.expect("no tree content").hg_content()?),
            None => Err(anyhow!("key {:?} not found in manifest", key)),
        }
    }

    fn get_content_iter(
        &self,
        keys: Vec<Key>,
        fetch_mode: FetchMode,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(Key, Bytes)>>> {
        let fetched = self.fetch_batch(keys.into_iter(), TreeAttributes::CONTENT, fetch_mode);
        let iter = fetched
            .into_iter()
            .map(|entry| -> anyhow::Result<(Key, Bytes)> {
                let (key, store_tree) = entry?;
                let content = store_tree
                    .content
                    .ok_or_else(|| anyhow::format_err!("no content available"))?;
                Ok((key, content.hg_content()?))
            });
        Ok(Box::new(iter))
    }

    fn prefetch(&self, keys: Vec<Key>) -> Result<()> {
        self.fetch_batch(
            keys.into_iter(),
            TreeAttributes::CONTENT,
            FetchMode::AllowRemote,
        )
        .consume();
        Ok(())
    }

    fn refresh(&self) -> Result<()> {
        TreeStore::refresh(self)
    }
}

/// Extends a basic `TreeEntry` with aux data.
struct ScmStoreTreeEntry {
    tree: LazyTree,
    // The "basic" version of `TreeEntry` that does not have aux data.
    basic_tree_entry: OnceCell<Box<dyn TreeEntry>>,
}

impl ScmStoreTreeEntry {
    fn basic_tree_entry(&self) -> Result<&dyn TreeEntry> {
        self.basic_tree_entry
            .get_or_try_init(|| {
                let data = self.tree.hg_content()?;
                let entry = storemodel::basic_parse_tree(data, SerializationFormat::Hg)?;
                Ok(entry)
            })
            .map(Borrow::borrow)
    }
}

impl TreeEntry for ScmStoreTreeEntry {
    fn iter(&self) -> Result<BoxIterator<Result<(PathComponentBuf, HgId, TreeItemFlag)>>> {
        self.basic_tree_entry()?.iter()
    }

    fn lookup(&self, name: &PathComponent) -> Result<Option<(HgId, TreeItemFlag)>> {
        self.basic_tree_entry()?.lookup(name)
    }

    fn file_aux_iter(&self) -> anyhow::Result<BoxIterator<anyhow::Result<(HgId, FileAuxData)>>> {
        let maybe_iter = (|| -> Option<BoxIterator<anyhow::Result<(HgId, FileAuxData)>>> {
            let entry = match &self.tree {
                LazyTree::SaplingRemoteApi(entry) => entry,
                _ => return None,
            };
            let children = entry.children.as_ref()?;
            let iter = children.iter().filter_map(|child| {
                let child = child.as_ref().ok()?;
                let file_entry = match child {
                    TreeChildEntry::File(v) => v,
                    _ => return None,
                };
                Some(Ok((
                    file_entry.key.hgid,
                    file_entry.file_metadata.clone()?.into(),
                )))
            });
            Some(Box::new(iter))
        })();
        Ok(maybe_iter.unwrap_or_else(|| Box::new(std::iter::empty())))
    }

    fn tree_aux_data_iter(
        &self,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(HgId, TreeAuxData)>>> {
        let maybe_iter = (|| -> Option<BoxIterator<anyhow::Result<(HgId, TreeAuxData)>>> {
            let entry = match &self.tree {
                LazyTree::SaplingRemoteApi(entry) => entry,
                // TODO: We should also support fetching tree metadata from local cache
                _ => return None,
            };
            let children = entry.children.as_ref()?;
            let iter = children.iter().filter_map(|child| {
                let child = child.as_ref().ok()?;
                let directory_entry = match child {
                    TreeChildEntry::Directory(v) => v,
                    _ => return None,
                };
                let tree_aux_data = directory_entry
                    .tree_aux_data
                    .ok_or_else(|| {
                        anyhow::anyhow!(format!(
                            "tree aux data is missing for key: {}",
                            directory_entry.key
                        ))
                    })
                    .ok()?;
                Some(Ok((directory_entry.key.hgid, tree_aux_data)))
            });
            Some(Box::new(iter))
        })();
        Ok(maybe_iter.unwrap_or_else(|| Box::new(std::iter::empty())))
    }

    /// Get the directory aux data of the tree.
    fn aux_data(&self) -> anyhow::Result<Option<TreeAuxData>> {
        let entry = match &self.tree {
            LazyTree::SaplingRemoteApi(entry) => entry,
            // TODO: We should also support fetching tree metadata from local cache
            _ => return Ok(None),
        };
        Ok(entry.tree_aux_data)
    }
}

impl storemodel::TreeStore for TreeStore {
    fn get_tree_iter(
        &self,
        keys: Vec<Key>,
        fetch_mode: FetchMode,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(Key, Box<dyn TreeEntry>)>>> {
        let fetched = self.fetch_batch(keys.into_iter(), TreeAttributes::CONTENT, fetch_mode);
        let iter = fetched
            .into_iter()
            .map(|entry| -> anyhow::Result<(Key, Box<dyn TreeEntry>)> {
                let (key, store_tree) = entry?;
                let tree: LazyTree = store_tree
                    .content
                    .ok_or_else(|| anyhow::format_err!("no content available"))?;
                // ScmStoreTreeEntry supports aux data.
                let tree_entry = ScmStoreTreeEntry {
                    tree,
                    basic_tree_entry: OnceCell::new(),
                };
                Ok((key, Box::new(tree_entry)))
            });
        Ok(Box::new(iter))
    }

    fn get_tree_aux_data_iter(
        &self,
        keys: Vec<Key>,
        fetch_mode: FetchMode,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(Key, TreeAuxData)>>> {
        let fetched = self.fetch_batch(keys.into_iter(), TreeAttributes::AUX_DATA, fetch_mode);
        let iter = fetched
            .into_iter()
            .map(|entry| -> anyhow::Result<(Key, TreeAuxData)> {
                let (key, store_tree) = entry?;
                let aux = store_tree
                    .aux_data
                    .ok_or_else(|| anyhow::anyhow!("aux data is missing from store tree"))?;
                Ok((key, aux))
            });
        Ok(Box::new(iter))
    }
}
