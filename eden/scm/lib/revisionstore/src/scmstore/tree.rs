/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Borrow;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use ::metrics::Counter;
use ::types::fetch_mode::FetchMode;
use ::types::hgid::NULL_ID;
use ::types::tree::TreeItemFlag;
use ::types::FetchContext;
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
use edenapi_types::FileAuxData;
use edenapi_types::TreeAuxData;
use fetch::FetchState;
use flume::bounded;
use flume::unbounded;
use minibytes::Bytes;
use once_cell::sync::OnceCell;
use progress_model::AggregatingProgressBar;
use progress_model::ProgressBar;
use progress_model::Registry;
use storemodel::BoxIterator;
use storemodel::InsertOpts;
use storemodel::KeyStore;
use storemodel::SerializationFormat;
use storemodel::TreeEntry;
use types::AuxData;

use crate::datastore::HgIdDataStore;
use crate::historystore::HistoryStore;
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
use crate::trait_impls::sha1_digest;
use crate::Delta;
use crate::HgIdHistoryStore;
use crate::HgIdMutableDeltaStore;
use crate::HgIdMutableHistoryStore;
use crate::IndexedLogHgIdHistoryStore;
use crate::LocalStore;
use crate::Metadata;
use crate::RemoteHistoryStore;
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

static TREESTORE_FLUSH_COUNT: Counter = Counter::new_counter("scmstore.tree.flush");

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

    pub format: SerializationFormat,

    // This bar "aggregates" across concurrent uses of this TreeStore from different
    // threads (so that only a single progress bar shows up to the user).
    pub(crate) progress_bar: Arc<AggregatingProgressBar>,

    // Temporary escape hatch to disable bounding of queue.
    pub(crate) unbounded_queue: bool,
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
        fctx: FetchContext,
        reqs: impl Iterator<Item = Key>,
        attrs: TreeAttributes,
    ) -> FetchResults<StoreTree> {
        let mut reqs = reqs.peekable();
        if reqs.peek().is_none() {
            return FetchResults::new(Box::new(std::iter::empty()));
        }

        // Unscientifically picked to be small enough to not use "all" the memory with a
        // full queue of (small) trees, but still generous enough to keep the pipeline
        // full of work for downstream consumers. The important thing is it is less than
        // infinity.
        const RESULT_QUEUE_SIZE: usize = 100_000;

        let bar = self.progress_bar.create_or_extend_local(0);

        let (found_tx, found_rx) = if self.unbounded_queue {
            // Escape hatch in case something goes wrong with bounding.
            unbounded()
        } else {
            // Bound channel size so we don't use unlimited memory queueing up file content
            // when the consumer is consuming slower than we are fetching.
            bounded(RESULT_QUEUE_SIZE)
        };

        let found_tx2 = found_tx.clone();
        let mut state = FetchState::new(reqs, attrs, found_tx, fctx.clone(), bar.clone());

        if tracing::enabled!(target: "tree_fetches", tracing::Level::TRACE) {
            let attrs = [
                attrs.content.then_some("content"),
                attrs.parents.then_some("parents"),
                attrs.aux_data.then_some("aux"),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();

            let mut keys = state.common.all_keys();
            keys.sort();
            let keys: Vec<_> = keys
                .into_iter()
                .map(|key| format!("{}@{}", key.path, &key.hgid.to_hex()[..8]))
                .collect();

            tracing::trace!(target: "tree_fetches", ?attrs, ?keys);
        }

        let keys_len = state.common.pending_len();

        bar.increase_total(keys_len as u64);

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
            TreeMetadataMode::OptIn => fctx.mode().contains(FetchMode::PREFETCH),
        };
        let fetch_tree_aux_data = self.fetch_tree_aux_data || attrs.aux_data;
        let fetch_parents = attrs.parents || self.prefetch_tree_parents;

        let fetch_local = fctx.mode().contains(FetchMode::LOCAL);
        let fetch_remote = fctx.mode().contains(FetchMode::REMOTE);

        tracing::debug!(
            ?fctx,
            ?attrs,
            fetch_children_metadata,
            fetch_tree_aux_data,
            fetch_local,
            fetch_remote,
            keys_len
        );

        let process_func = move || -> Result<()> {
            // We might be in a different thread than when `bar` was created - set bar as
            // active here as well.
            let _bar = ProgressBar::push_active(bar, Registry::main());

            // Handle queries for null tree id (with null content response). scmstore is
            // the end of the line, so if we consistently handle null id then callers at
            // any level can confidently assume null tree ids are handled.
            state
                .common
                .iter_pending(TreeAttributes::CONTENT, false, |key| {
                    if key.hgid.is_null() {
                        Some(StoreTree {
                            content: Some(LazyTree::Null),
                            parents: Some(Parents::None),
                            aux_data: None,
                        })
                    } else {
                        None
                    }
                });

            let fetch_cas = fetch_remote && cas_client.is_some();

            if fetch_local || fetch_cas {
                if let Some(tree_aux_store) = &tree_aux_store {
                    let mut wants_aux = TreeAttributes::AUX_DATA;
                    if cas_client.is_some() {
                        // We need the tree aux data in order to fetch from CAS, so fetch
                        // tree aux data for any key we want CONTENT for.
                        wants_aux |= TreeAttributes::CONTENT;
                    }
                    let pending: Vec<_> = state
                        .common
                        .pending(wants_aux, false)
                        .map(|(key, _attrs)| key.clone())
                        .collect();

                    let (mut found, mut miss) = (0, 0);
                    for key in pending.into_iter() {
                        if let Some(entry) = tree_aux_store.get(&key.hgid)? {
                            found += 1;

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
                        } else {
                            miss += 1;
                        }
                    }
                    state.metrics.aux.cache.hit(found);
                    state.metrics.aux.cache.miss(miss);
                }
            }

            let process_local = |state: &mut FetchState,
                                 log: &Option<Arc<IndexedLogHgIdDataStore>>,
                                 location|
             -> Result<()> {
                if let Some(log) = log {
                    let start_time = Instant::now();

                    let pending: Vec<_> = state
                        .common
                        .pending(TreeAttributes::CONTENT, false)
                        .map(|(key, _attrs)| key.clone())
                        .collect();

                    let bar = ProgressBar::new_adhoc("IndexedLog", pending.len() as u64, "trees");

                    let store_metrics = state.metrics.indexedlog.store(location);
                    let fetch_count = pending.len();

                    store_metrics.fetch(fetch_count);

                    let mut found_count: usize = 0;
                    for key in pending.into_iter() {
                        if let Some(entry) = log.get_entry(&key.hgid)? {
                            tracing::trace!("{:?} found in {:?}", key, location);
                            state.common.found(key, LazyTree::IndexedLog(entry).into());
                            found_count += 1;
                        }
                        bar.increase_position(1);
                    }

                    store_metrics.hit(found_count);
                    store_metrics.miss(fetch_count - found_count);
                    let _ = store_metrics.time_from_duration(start_time.elapsed());
                }

                Ok(())
            };

            if fetch_cas {
                // When fetching from CAS, first fetch from local repo to avoid network
                // request for data that is only available locally (e.g. locally
                // committed).
                if fetch_local {
                    process_local(&mut state, &indexedlog_local, StoreLocation::Local)?;
                }

                // Then fetch from CAS since we essentially always expect a hit.
                if let Some(cas_client) = &cas_client {
                    state.fetch_cas(cas_client, aux_cache.as_deref(), tree_aux_store.as_deref());
                }

                // Finally fetch from local cache (shouldn't normally get here).
                if fetch_local {
                    process_local(&mut state, &indexedlog_cache, StoreLocation::Cache)?;
                }
            } else if fetch_local {
                // If not using CAS, fetch from cache first then local (hit rate in cache
                // is typically much higher).
                process_local(&mut state, &indexedlog_cache, StoreLocation::Cache)?;
                process_local(&mut state, &indexedlog_local, StoreLocation::Local)?;
            }

            if fetch_local {
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

            if fetch_remote {
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

            Ok(())
        };
        let process_func_errors = move || {
            if let Err(err) = process_func() {
                let _ = found_tx2.send(Err(KeyFetchError::Other(err)));
            }
        };

        // Only kick off a thread if there's a substantial amount of work.
        if keys_len > 1000 {
            let active_bar = Registry::main().get_active_progress_bar();
            std::thread::spawn(move || {
                // Propagate parent progress bar into the thread so things nest well.
                Registry::main().set_active_progress_bar(active_bar);
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
                indexedlog_local.put_entry(Entry::new(key.hgid, bytes, meta))?;
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
            prefetch_tree_parents: false,
            format: SerializationFormat::Hg,
            progress_bar: AggregatingProgressBar::new("", ""),
            unbounded_queue: false,
        }
    }

    #[allow(unused_must_use)]
    #[tracing::instrument(level = "debug", skip(self))]
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

        TREESTORE_FLUSH_COUNT.increment();

        result
    }

    pub fn refresh(&self) -> Result<()> {
        self.flush()
    }

    pub fn with_shared_only(&self) -> Self {
        // this is infallible in ContentStore so panic if there are no shared/cache stores.
        assert!(
            self.indexedlog_cache.is_some(),
            "cannot get shared_mutable, no shared / local cache stores available"
        );
        Self {
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
            prefetch_tree_parents: false,
            format: self.format(),
            progress_bar: self.progress_bar.clone(),
            unbounded_queue: self.unbounded_queue,
        }
    }

    pub fn prefetch(&self, keys: Vec<Key>) -> Result<Vec<Key>> {
        Ok(self
            .fetch_batch(
                FetchContext::default(),
                keys.into_iter(),
                TreeAttributes::CONTENT,
            )
            .missing()?
            .into_iter()
            .collect())
    }
}

impl HgIdDataStore for TreeStore {
    // Fetch the raw content of a single TreeManifest blob
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        Ok(
            match self
                .fetch_batch(
                    FetchContext::default(),
                    std::iter::once(key.clone()).filter_map(StoreKey::maybe_into_key),
                    TreeAttributes::CONTENT,
                )
                .single()?
            {
                Some(entry) => StoreResult::Found(entry.content.expect("content attribute not found despite being requested and returned as complete").hg_content()?.into_vec()),
                None => StoreResult::NotFound(key),
            },
        )
    }

    fn refresh(&self) -> Result<()> {
        self.refresh()
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
            FetchContext::default(),
            std::iter::once(key.clone()),
            TreeAttributes::PARENTS,
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

impl RemoteHistoryStore for TreeStore {
    fn prefetch(&self, keys: &[StoreKey], _length: Option<u32>) -> Result<()> {
        self.fetch_batch(
            FetchContext::default(),
            keys.iter().filter_map(StoreKey::maybe_as_key).cloned(),
            TreeAttributes::PARENTS,
        )
        .missing()?;
        Ok(())
    }
}

impl HistoryStore for TreeStore {
    fn with_shared_only(&self) -> Arc<dyn HistoryStore> {
        Arc::new(self.with_shared_only())
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
                FetchContext::new(FetchMode::LocalOnly),
                std::iter::once(key.clone()),
                TreeAttributes::CONTENT,
            )
            .single()?
        {
            Some(entry) => Ok(Some(entry.content.expect("no tree content").hg_content()?)),
            None => Ok(None),
        }
    }

    fn get_content(
        &self,
        fctx: FetchContext,
        path: &RepoPath,
        node: Node,
    ) -> Result<minibytes::Bytes> {
        if node.is_null() {
            return Ok(Default::default());
        }
        let key = Key::new(path.to_owned(), node);
        match self
            .fetch_batch(fctx, std::iter::once(key.clone()), TreeAttributes::CONTENT)
            .single()?
        {
            Some(entry) => Ok(entry.content.expect("no tree content").hg_content()?),
            None => Err(anyhow!("key {:?} not found in manifest", key)),
        }
    }

    fn get_content_iter(
        &self,
        fctx: FetchContext,
        keys: Vec<Key>,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(Key, Bytes)>>> {
        let fetched = self.fetch_batch(fctx, keys.into_iter(), TreeAttributes::CONTENT);
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
            FetchContext::sapling_prefetch(),
            keys.into_iter(),
            TreeAttributes::CONTENT,
        )
        .consume();
        Ok(())
    }

    fn refresh(&self) -> Result<()> {
        TreeStore::refresh(self)
    }

    fn format(&self) -> SerializationFormat {
        self.format
    }

    fn flush(&self) -> anyhow::Result<()> {
        TreeStore::flush(self)
    }

    fn insert_data(&self, opts: InsertOpts, path: &RepoPath, data: &[u8]) -> anyhow::Result<HgId> {
        let id = sha1_digest(&opts, data, self.format());

        // PERF: Ideally there is no need to clone path or data.
        let key = Key::new(path.to_owned(), id);
        let data = Bytes::copy_from_slice(data);
        self.write_batch(std::iter::once((key, data, Default::default())))?;
        Ok(id)
    }

    fn clone_key_store(&self) -> Box<dyn KeyStore> {
        Box::new(self.clone())
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
        Ok(Box::new(
            self.tree
                .children_aux_data()
                .into_iter()
                .filter_map(|(hgid, aux)| match aux {
                    AuxData::File(file_aux_data) => Some(Ok((hgid, file_aux_data))),
                    AuxData::Tree(_) => None,
                }),
        ))
    }

    fn tree_aux_data_iter(
        &self,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(HgId, TreeAuxData)>>> {
        Ok(Box::new(
            self.tree
                .children_aux_data()
                .into_iter()
                .filter_map(|(hgid, aux)| match aux {
                    AuxData::File(_) => None,
                    AuxData::Tree(tree_aux_data) => Some(Ok((hgid, tree_aux_data))),
                }),
        ))
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
        fctx: FetchContext,
        keys: Vec<Key>,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(Key, Box<dyn TreeEntry>)>>> {
        let fetched = self.fetch_batch(fctx, keys.into_iter(), TreeAttributes::CONTENT);
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
        fctx: FetchContext,
        keys: Vec<Key>,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(Key, TreeAuxData)>>> {
        let fetched = self.fetch_batch(fctx, keys.into_iter(), TreeAttributes::AUX_DATA);
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

    fn clone_tree_store(&self) -> Box<dyn storemodel::TreeStore> {
        Box::new(self.clone())
    }
}
