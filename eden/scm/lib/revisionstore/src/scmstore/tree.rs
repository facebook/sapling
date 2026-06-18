/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Borrow;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use ::metrics::Counter;
use ::types::FetchContext;
use ::types::HgId;
use ::types::Key;
use ::types::Node;
use ::types::NodeInfo;
use ::types::Parents;
use ::types::PathComponent;
use ::types::PathComponentBuf;
use ::types::RepoPath;
use ::types::fetch_mode::FetchMode;
use ::types::hgid::NULL_ID;
use ::types::tree::TreeItemFlag;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use blob::Blob;
use edenapi_types::CheckManifestPermissionRequest;
use edenapi_types::FileAuxData;
use edenapi_types::TreeAuxData;
use fetch::FetchState;
use flume::bounded;
use flume::unbounded;
use metrics::TREE_STORE_FETCH_METRICS;
use minibytes::Bytes;
use moka::sync::Cache;
use once_cell::sync::OnceCell;
use progress_model::AggregatingProgressBar;
use progress_model::ProgressBar;
use progress_model::Registry;
use storemodel::BoxIterator;
use storemodel::BoxRefIterator;
use storemodel::InsertOpts;
use storemodel::KeyStore;
use storemodel::SerializationFormat;
use storemodel::TreeEntry;
use storemodel::TreeFetchItems;
use storemodel::basic_parse_tree;
use types::AuxData;

use super::util::try_local_content;
use super::util::try_local_entry;
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
use crate::datastore::HgIdDataStore;
use crate::historystore::HistoryStore;
use crate::indexedlogdatastore::Entry;
use crate::indexedlogdatastore::IndexedLogHgIdDataStore;
use crate::indexedlogtreeauxstore::TreeAuxStore;
use crate::scmstore::fetch::FetchResults;
use crate::scmstore::fetch::KeyFetchError;
use crate::scmstore::fetch::MaxFetchCount;
use crate::scmstore::file::FileStore;
use crate::scmstore::metrics::StoreLocation;
use crate::scmstore::tree::types::LazyTree;
use crate::scmstore::tree::types::StoreTree;
use crate::scmstore::tree::types::TreeAttributes;
use crate::trait_impls::sha1_digest;

mod fetch;
mod metrics;
pub mod types;

#[derive(Clone, Debug)]
pub enum TreeMetadataMode {
    Never,
    Always,
    OptIn,
}

/// Controls how path-based ACL restrictions are handled for tree entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestrictedTreeMode {
    /// No ACL checking. permission_denied_children() returns empty.
    Disabled,
    /// ACL checks are performed and results logged, but not enforced.
    Logged,
    /// ACL checks are performed, logged, and enforced.
    Enforced,
}

static TREESTORE_FLUSH_COUNT: Counter = Counter::new_counter("scmstore.tree.flush");
const ACL_CHECK_CACHE_TTL: Duration = Duration::from_secs(60);
const ACL_CHECK_CACHE_MAX_ENTRIES: u64 = 4096;

pub(crate) type AclCheckCache = Cache<HgId, AclCheckResult>;

#[derive(Clone)]
pub(crate) enum AclCheckResult {
    Allowed,
    Denied(String),
}

pub(crate) fn new_acl_check_cache() -> AclCheckCache {
    Cache::builder()
        .max_capacity(ACL_CHECK_CACHE_MAX_ENTRIES)
        .time_to_live(ACL_CHECK_CACHE_TTL)
        .build()
}

#[derive(Debug, Clone)]
pub struct TreeEntryWithAux {
    entry: Entry,
    tree_aux: Option<TreeAuxData>,
}

impl TreeEntryWithAux {
    pub fn content(&self) -> Result<Bytes> {
        self.entry.content()
    }

    pub fn node(&self) -> HgId {
        self.entry.node()
    }
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

    pub(crate) verify_hash: bool,

    pub restricted_tree_mode: RestrictedTreeMode,

    pub(crate) acl_check_cache: AclCheckCache,

    pub(crate) permission_denied_paths:
        Option<Arc<parking_lot::Mutex<VecDeque<::types::errors::PermissionDenied>>>>,

    // Bounds the number of items this store can deliver across the lifetime of
    // the process. When exceeded, every subsequent item becomes an error,
    // catching all callers and code paths (including serial fetches). Set via
    // `TreeStoreBuilder::max_fetch_count`; absent means the guard is disabled.
    pub(crate) max_fetch_count: MaxFetchCount,
}

impl Drop for TreeStore {
    fn drop(&mut self) {
        if self.flush_on_drop {
            let _ = TreeStore::flush(self);
        }
    }
}

impl TreeStore {
    pub(crate) fn get_local_content_direct(&self, id: &HgId) -> Result<Option<Blob>> {
        let m = &TREE_STORE_FETCH_METRICS;

        tracing::trace!(target: "tree_fetches", %id, "direct_content");

        try_local_content!(id, self.indexedlog_cache, m.indexedlog.cache);
        try_local_content!(id, self.indexedlog_local, m.indexedlog.local);

        Ok(None)
    }

    pub(crate) fn get_indexedlog_entry_direct(&self, id: &HgId) -> anyhow::Result<Option<Entry>> {
        let m = &TREE_STORE_FETCH_METRICS;
        try_local_entry!(id, self.indexedlog_cache, m.indexedlog.cache);
        try_local_entry!(id, self.indexedlog_local, m.indexedlog.local);
        Ok(None)
    }

    /// Fetch a tree from the local caches. If the tree is not found, return None.
    pub fn get_local_tree_direct(&self, node: HgId) -> anyhow::Result<Option<Arc<dyn TreeEntry>>> {
        if node.is_null() {
            return Ok(Some(basic_parse_tree(Bytes::default(), self.format())?));
        }

        match self.get_indexedlog_entry_direct(&node)? {
            None => Ok(None),
            Some(entry) => {
                let res: Arc<ScmStoreTreeEntry> = Arc::new(ScmStoreTreeEntry {
                    tree: LazyTree::IndexedLog(
                        TreeEntryWithAux {
                            entry,
                            tree_aux: self.get_local_aux_direct(&node)?,
                        },
                        self.format(),
                    ),
                    basic_tree_entry: OnceCell::new(),
                    acl_checker: self.create_acl_checker(),
                });
                Ok(Some(res))
            }
        }
    }

    pub(crate) fn get_local_aux_direct(&self, id: &HgId) -> Result<Option<TreeAuxData>> {
        let m = &TREE_STORE_FETCH_METRICS.aux.cache;
        if let Some(store) = &self.tree_aux_store {
            m.requests.increment();
            m.keys.increment();
            m.singles.increment();
            match store.get(id) {
                Ok(None) => {
                    m.misses.increment();
                }
                Ok(Some(data)) => {
                    m.hits.increment();
                    return Ok(Some(data));
                }
                Err(err) => {
                    m.errors.increment();
                    return Err(err);
                }
            }
        }
        Ok(None)
    }

    /// Create a deferred ACL checker closure that captures the edenapi client.
    /// Returns None if no edenapi client is configured or mode is Disabled.
    fn create_acl_checker(&self) -> Option<AclChecker> {
        if self.restricted_tree_mode == RestrictedTreeMode::Disabled {
            return None;
        }
        let edenapi = self.edenapi.clone()?;
        let mode = self.restricted_tree_mode;
        let acl_check_cache = self.acl_check_cache.clone();
        Some(Arc::new(
            move |children_with_acl: Vec<(PathComponentBuf, HgId)>| {
                let mut denied_map: HashMap<HgId, String> = HashMap::new();

                let manifest_ids = children_with_acl
                    .iter()
                    .filter_map(|(_, hgid)| match acl_check_cache.get(hgid) {
                        Some(AclCheckResult::Denied(acl)) => {
                            denied_map.insert(*hgid, acl);
                            None
                        }
                        Some(AclCheckResult::Allowed) => None,
                        None => Some(*hgid),
                    })
                    .collect::<Vec<_>>();

                if !manifest_ids.is_empty() {
                    let request = CheckManifestPermissionRequest { manifest_ids };
                    let response = edenapi.check_manifest_permission_blocking(request)?;

                    for resp in response.entries {
                        let result = if resp.has_access {
                            AclCheckResult::Allowed
                        } else {
                            let acl = resp
                                .request_acl
                                .unwrap_or_else(|| "unknown-acl".to_string());
                            denied_map.insert(resp.manifest_id, acl.clone());
                            AclCheckResult::Denied(acl)
                        };
                        acl_check_cache.insert(resp.manifest_id, result);
                    }
                }

                if mode == RestrictedTreeMode::Logged {
                    for (path, hgid) in &children_with_acl {
                        if let Some(acl) = denied_map.get(hgid) {
                            tracing::info!(
                                %path, %hgid, %acl,
                                "restricted tree detected (logged mode, not enforcing)"
                            );
                        }
                    }
                    return Ok(Box::new(std::iter::empty())
                        as BoxIterator<anyhow::Result<(PathComponentBuf, HgId, String)>>);
                }
                let iter = children_with_acl
                    .into_iter()
                    .filter_map(move |(path, hgid)| {
                        denied_map
                            .get(&hgid)
                            .map(|acl| Ok((path, hgid, acl.clone())))
                    });
                Ok(Box::new(iter)
                    as BoxIterator<
                        anyhow::Result<(PathComponentBuf, HgId, String)>,
                    >)
            },
        ))
    }

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

        let indexedlog_cache = self.indexedlog_cache.clone();
        let aux_cache = self.filestore.as_ref().and_then(|fs| fs.aux_cache.clone());
        let tree_aux_store = self.tree_aux_store.clone();

        let found_tx2 = found_tx.clone();
        let mut state = FetchState::new(
            reqs,
            attrs,
            found_tx,
            fctx.clone(),
            bar.clone(),
            indexedlog_cache.clone(),
            aux_cache,
            tree_aux_store.clone(),
            self.max_fetch_count.clone(),
        );

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

        let indexedlog_local = self.indexedlog_local.clone();
        let edenapi = self.edenapi.clone();

        let historystore_cache = self.historystore_cache.clone();
        let historystore_local = self.historystore_local.clone();

        let cache_to_local_cache = self.cache_to_local_cache;

        let fetch_children_metadata = match self.tree_metadata_mode {
            TreeMetadataMode::Always => true,
            TreeMetadataMode::Never => false,
            TreeMetadataMode::OptIn => fctx.mode().contains(FetchMode::PREFETCH),
        };
        let fetch_tree_aux_data = self.fetch_tree_aux_data || attrs.aux_data;
        let fetch_parents = attrs.parents || self.prefetch_tree_parents;

        let fetch_local = fctx.mode().contains(FetchMode::LOCAL);
        let fetch_remote = fctx.mode().contains(FetchMode::REMOTE);
        let sync_mode = fctx.sync_mode();

        tracing::debug!(
            ?fctx,
            ?attrs,
            fetch_children_metadata,
            fetch_tree_aux_data,
            fetch_local,
            fetch_remote,
            keys_len
        );

        let verify_hash = self.verify_hash;
        let format = self.format();
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

            if fetch_local {
                if let Some(tree_aux_store) = &tree_aux_store {
                    let (mut found, mut miss, mut errors) = (0, 0, 0);
                    state
                        .common
                        .iter_pending(TreeAttributes::AUX_DATA, false, |key| match tree_aux_store
                            .get(&key.hgid)
                        {
                            Ok(Some(entry)) => {
                                found += 1;

                                tracing::trace!(?key, ?entry, "found tree aux entry in cache");
                                Some(StoreTree {
                                    content: None,
                                    parents: None,
                                    aux_data: Some(entry),
                                })
                            }
                            Ok(None) => {
                                miss += 1;
                                None
                            }
                            Err(err) => {
                                errors += 1;
                                state.errors.keyed_error(key.clone(), err);
                                None
                            }
                        });
                    state.metrics.aux.cache.hit(found);
                    state.metrics.aux.cache.miss(miss);
                    state.metrics.aux.cache.err(errors);
                }
            }

            let process_local = |state: &mut FetchState,
                                 log: &Option<Arc<IndexedLogHgIdDataStore>>,
                                 location|
             -> Result<()> {
                if let Some(log) = log {
                    let start_time = Instant::now();

                    let bar = ProgressBar::new_adhoc("IndexedLog", 0, "trees");

                    let store_metrics = state.metrics.indexedlog.store(location);

                    let mut fetch_count: usize = 0;
                    let mut found_count: usize = 0;
                    let mut errors_count: usize = 0;
                    state
                        .common
                        .iter_pending(TreeAttributes::CONTENT, false, |key| {
                            fetch_count += 1;
                            bar.increase_position(1);
                            match log.get_entry(&key.hgid) {
                                Ok(Some(entry)) => {
                                    tracing::trace!("{:?} found in {:?}", key, location);
                                    found_count += 1;
                                    Some(
                                        LazyTree::IndexedLog(
                                            TreeEntryWithAux {
                                                entry,
                                                tree_aux: None,
                                            },
                                            format,
                                        )
                                        .into(),
                                    )
                                }
                                Ok(None) => None,
                                Err(err) => {
                                    errors_count += 1;
                                    state.errors.keyed_error(key.clone(), err);
                                    None
                                }
                            }
                        });

                    store_metrics.fetch(fetch_count);
                    store_metrics.hit(found_count);
                    store_metrics.miss(fetch_count - found_count);
                    store_metrics.err(errors_count);
                    let _ = store_metrics.time_from_duration(start_time.elapsed());
                }

                Ok(())
            };

            if fetch_local {
                // Fetch from cache first then local (hit rate in cache is typically much higher).
                process_local(&mut state, &indexedlog_cache, StoreLocation::Cache)?;
                process_local(&mut state, &indexedlog_local, StoreLocation::Local)?;

                for (name, log) in [
                    ("cache", &historystore_cache),
                    ("local", &historystore_local),
                ] {
                    if let Some(log) = log {
                        state
                            .common
                            .iter_pending(TreeAttributes::PARENTS, false, |key| {
                                match log.get_node_info(key) {
                                    Ok(Some(entry)) => {
                                        tracing::trace!("{:?} found parents in {name}", key);
                                        Some(StoreTree {
                                            content: None,
                                            parents: Some(Parents::new(
                                                entry.parents[0].hgid,
                                                entry.parents[1].hgid,
                                            )),
                                            aux_data: None,
                                        })
                                    }
                                    Ok(None) => None,
                                    Err(err) => {
                                        state.errors.keyed_error(key.clone(), err);
                                        None
                                    }
                                }
                            });
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
                        if fetch_parents {
                            historystore_cache.as_deref()
                        } else {
                            None
                        },
                        verify_hash,
                        format,
                    )?;
                } else {
                    tracing::debug!("no SaplingRemoteApi associated with TreeStore");
                }
            }

            // We made it to the end with no overall errors - report_missing=true so we report errors
            // for any items we unexpectedly didn't get results for.
            state
                .common
                .results(std::mem::take(&mut state.errors), true);

            Ok(())
        };
        let process_func_errors = move || {
            if let Err(err) = process_func() {
                let _ = found_tx2.send(Err(KeyFetchError::Other(err)));
            }
        };

        // Only kick off a thread if there's a substantial amount of work.
        if sync_mode.should_spawn(keys_len) {
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
            verify_hash: true,
            restricted_tree_mode: RestrictedTreeMode::Disabled,
            acl_check_cache: new_acl_check_cache(),
            permission_denied_paths: Default::default(),
            max_fetch_count: Default::default(),
        }
    }

    #[allow(unused_must_use)]
    #[tracing::instrument(level = "debug", skip(self))]
    pub fn flush(&self) -> Result<()> {
        self.flush_inner(true)
    }

    pub fn sync(&self) -> Result<()> {
        self.flush_inner(false)
    }

    fn flush_inner(&self, skip_clean: bool) -> Result<()> {
        let mut result = Ok(());
        let mut handle_error = |error| {
            tracing::error!(%error);
            result = Err(error);
        };

        if let Some(ref indexedlog_local) = self.indexedlog_local {
            if !skip_clean || indexedlog_local.is_dirty() {
                indexedlog_local.flush_log().map_err(&mut handle_error).ok();
            }
        }

        if let Some(ref indexedlog_cache) = self.indexedlog_cache {
            if !skip_clean || indexedlog_cache.is_dirty() {
                indexedlog_cache.flush_log().map_err(&mut handle_error).ok();
            }
        }

        if let Some(ref tree_aux_store) = self.tree_aux_store {
            if !skip_clean || tree_aux_store.is_dirty() {
                tree_aux_store.flush().map_err(&mut handle_error).ok();
            }
        }

        if let Some(ref historystore_local) = self.historystore_local {
            if !skip_clean || historystore_local.is_dirty() {
                historystore_local.flush().map_err(&mut handle_error).ok();
            }
        }

        if let Some(ref historystore_cache) = self.historystore_cache {
            if !skip_clean || historystore_cache.is_dirty() {
                historystore_cache.flush().map_err(&mut handle_error).ok();
            }
        }

        TREESTORE_FLUSH_COUNT.increment();

        result
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
            filestore: None,
            tree_aux_store: None,
            flush_on_drop: true,
            tree_metadata_mode: TreeMetadataMode::Never,
            fetch_tree_aux_data: false,
            prefetch_tree_parents: false,
            format: self.format(),
            progress_bar: self.progress_bar.clone(),
            unbounded_queue: self.unbounded_queue,
            verify_hash: self.verify_hash,
            restricted_tree_mode: self.restricted_tree_mode,
            acl_check_cache: self.acl_check_cache.clone(),
            permission_denied_paths: self.permission_denied_paths.clone(),
            max_fetch_count: self.max_fetch_count.clone(),
        }
    }

    pub fn prefetch(&self, keys: Vec<Key>) -> Result<Vec<Key>> {
        Ok(self
            .fetch_batch(
                FetchContext::sapling_prefetch(),
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

    fn sync(&self) -> Result<()> {
        self.sync()
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

    fn sync(&self) -> Result<()> {
        self.sync()
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
            FetchContext::sapling_prefetch(),
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
    fn get_local_content(&self, _path: &RepoPath, node: HgId) -> anyhow::Result<Option<Blob>> {
        if node.is_null() {
            return Ok(Some(Blob::Bytes(Default::default())));
        }
        self.get_local_content_direct(&node)
    }

    fn get_content(&self, mut fctx: FetchContext, path: &RepoPath, node: Node) -> Result<Blob> {
        if node.is_null() {
            return Ok(Blob::Bytes(Default::default()));
        }

        // This path is hot for code paths such as manifest-tree iter/diff. They tend to do big prefetches and then do single fetches.
        // Optimize the single fetches by optimistically checking local caches directly (which skips the overhead of fetch_batch).
        if fctx.mode().contains(FetchMode::LOCAL) {
            if let Some(blob) = self.get_local_content(path, node)? {
                return Ok(blob);
            }

            // Don't need to check local anymore, so remove from fetch mode.
            fctx =
                FetchContext::new_with_mode_and_cause(fctx.mode() - FetchMode::LOCAL, fctx.cause())
                    .with_sync_mode(fctx.sync_mode());
        }

        let key = Key::new(path.to_owned(), node);
        match self
            .fetch_batch(fctx, std::iter::once(key.clone()), TreeAttributes::CONTENT)
            .single()?
        {
            Some(entry) => Ok(Blob::Bytes(
                entry.content.expect("no tree content").hg_content()?,
            )),
            None => Err(anyhow!("key {key:?} not found in manifest")),
        }
    }

    fn get_content_iter(
        &self,
        fctx: FetchContext,
        keys: Vec<Key>,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(Key, Blob)>>> {
        let fetched = self.fetch_batch(fctx, keys.into_iter(), TreeAttributes::CONTENT);
        let iter = fetched
            .into_iter()
            .map(|entry| -> anyhow::Result<(Key, Blob)> {
                let (key, store_tree) = entry?;
                let content = store_tree
                    .content
                    .ok_or_else(|| anyhow::format_err!("no content available"))?;
                Ok((key, Blob::Bytes(content.hg_content()?)))
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

    fn sync(&self) -> Result<()> {
        TreeStore::sync(self)
    }

    fn format(&self) -> SerializationFormat {
        self.format
    }

    fn flush(&self) -> anyhow::Result<()> {
        TreeStore::flush(self)
    }

    fn insert_data(&self, opts: InsertOpts, path: &RepoPath, data: Blob) -> anyhow::Result<HgId> {
        let data = data.to_bytes();
        let id = sha1_digest(&opts, &data, self.format());

        // For non-permanent inserts, prefer the cache store over local.
        // Fall back to local if cache is not available.
        let target_store = if opts.permanent {
            self.indexedlog_local.as_deref()
        } else {
            self.indexedlog_cache
                .as_deref()
                .or(self.indexedlog_local.as_deref())
        };

        if opts.read_before_write {
            if let Some(store) = target_store {
                if IndexedLogHgIdDataStore::contains(store, &id)? {
                    return Ok(id);
                }
            }
        }

        // PERF: Ideally there is no need to clone path or data.
        let key = Key::new(path.to_owned(), id);

        // Write parent info to the history store.
        // Fall back to local if cache is not available.
        let historystore = if opts.permanent {
            self.historystore_local.as_deref()
        } else {
            self.historystore_cache
                .as_deref()
                .or(self.historystore_local.as_deref())
        };
        if let Some(historystore) = historystore {
            let p1 = opts.parents.first().copied().unwrap_or(NULL_ID);
            let p2 = opts.parents.get(1).copied().unwrap_or(NULL_ID);
            let info = NodeInfo {
                parents: [
                    Key::new(key.path.clone(), p1),
                    Key::new(key.path.clone(), p2),
                ],
                linknode: NULL_ID,
            };
            historystore.add(&key, &info)?;
        }

        let mut entry = Entry::new(key.hgid, data, Default::default());
        if let Some(indices) = opts.acl_children_indices {
            if !indices.is_empty() {
                entry.set_acl_children_indices(indices);
            }
        }

        match target_store {
            Some(store) => store.put_entry(entry)?,
            None => bail!("no local or cache store to insert tree data into"),
        }

        Ok(id)
    }

    fn clone_key_store(&self) -> Box<dyn KeyStore> {
        Box::new(self.clone())
    }
}

/// Type alias for the deferred ACL checker callback. Given a list of
/// (path_component, manifest_id) for children with has_acl, returns denied
/// results. The outer Result captures transport/batch errors; the inner
/// iterator yields per-entry results.
type AclChecker = Arc<
    dyn Fn(
            Vec<(PathComponentBuf, HgId)>,
        ) -> anyhow::Result<BoxIterator<anyhow::Result<(PathComponentBuf, HgId, String)>>>
        + Send
        + Sync,
>;

/// Extends a basic `TreeEntry` with aux data and deferred ACL checking.
struct ScmStoreTreeEntry {
    tree: LazyTree,
    // The "basic" version of `TreeEntry` that does not have aux data.
    basic_tree_entry: OnceCell<Arc<dyn TreeEntry>>,
    // Deferred ACL checker callback. Called at permission_denied_children() time.
    acl_checker: Option<AclChecker>,
}

impl ScmStoreTreeEntry {
    fn basic_tree_entry(&self) -> Result<&dyn TreeEntry> {
        self.basic_tree_entry
            .get_or_try_init(|| Ok(Arc::new(self.tree.manifest_tree_entry()?)))
            .map(Borrow::borrow)
    }
}

impl From<LazyTree> for ScmStoreTreeEntry {
    fn from(tree: LazyTree) -> Self {
        ScmStoreTreeEntry {
            tree,
            basic_tree_entry: OnceCell::new(),
            acl_checker: None,
        }
    }
}

impl TreeEntry for ScmStoreTreeEntry {
    fn iter<'a>(
        &'a self,
    ) -> Result<BoxRefIterator<'a, Result<(&'a PathComponent, HgId, TreeItemFlag)>>> {
        self.basic_tree_entry()?.iter()
    }

    fn iter_owned(&self) -> Result<BoxIterator<Result<(PathComponentBuf, HgId, TreeItemFlag)>>> {
        self.basic_tree_entry()?.iter_owned()
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
        Ok(self.tree.aux_data())
    }

    fn filter_permission_denied(
        &self,
        children_with_acl: Vec<(PathComponentBuf, HgId)>,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(PathComponentBuf, HgId, String)>>> {
        let acl_checker = match &self.acl_checker {
            Some(c) => c.clone(),
            None => return Ok(Box::new(std::iter::empty())),
        };

        if children_with_acl.is_empty() {
            return Ok(Box::new(std::iter::empty()));
        }

        acl_checker(children_with_acl)
    }

    fn children_with_acls(&self) -> Result<Vec<(PathComponentBuf, HgId)>> {
        self.tree.children_with_acl()
    }

    fn size_hint(&self) -> Option<usize> {
        match &self.tree {
            LazyTree::IndexedLog(..) => self
                .basic_tree_entry()
                .map(|t| t.size_hint())
                .unwrap_or_default(),
            LazyTree::SaplingRemoteApi(slapi, ..) => slapi.children.as_ref().map(|c| c.len()),
            LazyTree::Null => Some(0),
        }
    }
}

impl storemodel::TreeStore for TreeStore {
    fn get_local_tree(
        &self,
        _path: &RepoPath,
        id: HgId,
    ) -> anyhow::Result<Option<Arc<dyn TreeEntry>>> {
        self.get_local_tree_direct(id)
    }

    fn get_tree_iter(&self, fctx: FetchContext, keys: Vec<Key>) -> anyhow::Result<TreeFetchItems> {
        // TreeAttributes::CONTENT means at least the content attribute is requested.
        // In practice, files/trees aux data may be requested as well, but we don't know that here as it depends on the configs.
        let fetched = self.fetch_batch(fctx, keys.into_iter(), TreeAttributes::CONTENT);
        let acl_checker = self.create_acl_checker();
        let iter =
            fetched
                .into_iter()
                .map(move |entry| -> anyhow::Result<(Key, Arc<dyn TreeEntry>)> {
                    let (key, store_tree) = entry?;
                    let tree: LazyTree = store_tree
                        .content
                        .ok_or_else(|| anyhow::format_err!("no content available"))?;
                    let mut scm_entry: ScmStoreTreeEntry = tree.into();
                    scm_entry.acl_checker = acl_checker.clone();
                    Ok((key, Arc::new(scm_entry) as Arc<dyn TreeEntry>))
                });
        Ok(TreeFetchItems::item_stream(iter))
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
                    .aux_data()
                    .ok_or_else(|| anyhow::anyhow!("aux data is missing from store tree"))?;
                Ok((key, aux))
            });
        Ok(Box::new(iter))
    }

    fn clone_tree_store(&self) -> Box<dyn storemodel::TreeStore> {
        Box::new(self.clone())
    }

    fn get_local_tree_aux_data(
        &self,
        _path: &RepoPath,
        id: HgId,
    ) -> anyhow::Result<Option<TreeAuxData>> {
        self.get_local_aux_direct(&id)
    }

    fn record_permission_denied(&self, err: ::types::errors::PermissionDenied) {
        if let Some(paths) = &self.permission_denied_paths {
            let mut denied = paths.lock();
            if denied.len() >= 1000 {
                denied.pop_front();
            }
            denied.push_back(err);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use minibytes::Bytes;
    use storemodel::InsertOpts;
    use storemodel::KeyStore;
    use storemodel::Kind;
    use storemodel::SerializationFormat;
    use tempfile::TempDir;
    use types::HgId;
    use types::RepoPathBuf;

    use crate::Metadata;
    use crate::StoreType;
    use crate::ToKeys;
    use crate::indexedlogdatastore::IndexedLogHgIdDataStore;
    use crate::indexedlogdatastore::IndexedLogHgIdDataStoreConfig;
    use crate::scmstore::tree::TreeStore;

    fn make_data_store(tempdir: &TempDir) -> Arc<IndexedLogHgIdDataStore> {
        let config = IndexedLogHgIdDataStoreConfig {
            max_log_count: None,
            max_bytes_per_log: None,
            max_bytes: None,
            btrfs_compression: false,
        };
        Arc::new(
            IndexedLogHgIdDataStore::new(
                &BTreeMap::<&str, &str>::new(),
                tempdir,
                &config,
                StoreType::Rotated,
                SerializationFormat::Hg,
            )
            .unwrap(),
        )
    }

    #[test]
    fn test_insert_data_read_before_write() {
        let tempdir = TempDir::new().unwrap();
        let indexedlog = make_data_store(&tempdir);

        let mut store = TreeStore::empty();
        store.indexedlog_local = Some(indexedlog.clone());

        let path = RepoPathBuf::from_string("foo".to_string()).unwrap();
        let data: &'static [u8] = b"tree data";

        // First insert without read_before_write.
        let opts = InsertOpts {
            kind: Kind::Tree,
            ..Default::default()
        };
        let id1 = store.insert_data(opts, &path, data.into()).unwrap();

        assert!(indexedlog.contains(&id1).unwrap());
        assert_eq!(indexedlog.to_keys().len(), 1);

        // Second insert with read_before_write=true should skip the write.
        let opts = InsertOpts {
            kind: Kind::Tree,
            read_before_write: true,
            ..Default::default()
        };
        let id2 = store.insert_data(opts, &path, data.into()).unwrap();

        assert_eq!(id1, id2);
        assert_eq!(indexedlog.to_keys().len(), 1);

        // Third insert with read_before_write=false writes a duplicate.
        let opts = InsertOpts {
            kind: Kind::Tree,
            read_before_write: false,
            ..Default::default()
        };
        let id3 = store.insert_data(opts, &path, data.into()).unwrap();

        assert_eq!(id1, id3);
        assert_eq!(indexedlog.to_keys().len(), 2);
    }

    #[test]
    fn test_insert_data_permanent_routing() {
        let local_dir = TempDir::new().unwrap();
        let cache_dir = TempDir::new().unwrap();

        let local = make_data_store(&local_dir);
        let cache = make_data_store(&cache_dir);

        let mut store = TreeStore::empty();
        store.indexedlog_local = Some(local.clone());
        store.indexedlog_cache = Some(cache.clone());

        let path = RepoPathBuf::from_string("foo".to_string()).unwrap();

        // Non-permanent insert goes to cache.
        let opts = InsertOpts {
            kind: Kind::Tree,
            permanent: false,
            ..Default::default()
        };
        store.insert_data(opts, &path, b"data1"[..].into()).unwrap();
        assert_eq!(cache.to_keys().len(), 1);
        assert_eq!(local.to_keys().len(), 0);

        // Permanent (default) insert goes to local.
        let opts = InsertOpts {
            kind: Kind::Tree,
            ..Default::default()
        };
        store.insert_data(opts, &path, b"data2"[..].into()).unwrap();
        assert_eq!(cache.to_keys().len(), 1);
        assert_eq!(local.to_keys().len(), 1);
    }

    #[test]
    fn test_insert_data_writes_parents_to_history_store() {
        use types::HgId;

        use crate::HgIdHistoryStore;
        use crate::IndexedLogHgIdHistoryStore;

        let data_dir = TempDir::new().unwrap();
        let history_dir = TempDir::new().unwrap();

        let indexedlog = make_data_store(&data_dir);
        let historystore = Arc::new(
            IndexedLogHgIdHistoryStore::new(
                &history_dir,
                &BTreeMap::<&str, &str>::new(),
                StoreType::Rotated,
            )
            .unwrap(),
        );

        let mut store = TreeStore::empty();
        store.indexedlog_local = Some(indexedlog);
        store.historystore_local = Some(historystore.clone());

        let path = RepoPathBuf::from_string("foo".to_string()).unwrap();
        let data: &'static [u8] = b"tree data";
        let p1 = HgId::from_hex(b"1111111111111111111111111111111111111111").unwrap();
        let p2 = HgId::from_hex(b"2222222222222222222222222222222222222222").unwrap();

        // Insert with parents.
        let opts = InsertOpts {
            kind: Kind::Tree,
            parents: vec![p1, p2],
            ..Default::default()
        };
        let id = store.insert_data(opts, &path, data.into()).unwrap();

        // Verify parent info was written to the history store.
        let key = types::Key::new(path.clone(), id);
        let info = historystore.get_node_info(&key).unwrap().unwrap();
        assert_eq!(info.parents[0].hgid, p1);
        assert_eq!(info.parents[1].hgid, p2);
    }

    #[test]
    fn test_get_local_tree_acl_children_roundtrip() {
        let tempdir = TempDir::new().unwrap();
        let indexedlog = make_data_store(&tempdir);

        let child_dir_id = HgId::from_hex(b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        let child_file_id = HgId::from_hex(b"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap();

        // Build an Hg tree blob: "dir\0<hex>t\nfile\0<hex>\n"
        let tree_data = format!(
            "dir\0{}t\nfile\0{}\n",
            child_dir_id.to_hex(),
            child_file_id.to_hex(),
        );
        let tree_bytes = Bytes::copy_from_slice(tree_data.as_bytes());

        let tree_id = HgId::from_hex(b"cccccccccccccccccccccccccccccccccccccccc").unwrap();
        let mut entry =
            crate::indexedlogdatastore::Entry::new(tree_id, tree_bytes, Metadata::default());
        entry.set_acl_children_indices(vec![0]); // index 0 = "dir"
        indexedlog.put_entry(entry).unwrap();
        indexedlog.flush_log().unwrap();

        let mut store = TreeStore::empty();
        store.indexedlog_local = Some(indexedlog);
        let tree_entry = store.get_local_tree_direct(tree_id).unwrap().unwrap();

        let acl = tree_entry.children_with_acls().unwrap();
        assert_eq!(acl.len(), 1);
        assert_eq!(acl[0].0.as_str(), "dir");
        assert_eq!(acl[0].1, child_dir_id);
    }

    #[test]
    fn test_get_indexedlog_entry_preserves_acl_children_indices() {
        let tempdir = TempDir::new().unwrap();
        let indexedlog = make_data_store(&tempdir);

        let tree_id = HgId::from_hex(b"cccccccccccccccccccccccccccccccccccccccc").unwrap();
        let mut entry = crate::indexedlogdatastore::Entry::new(
            tree_id,
            Bytes::from_static(b"tree data"),
            Metadata::default(),
        );
        entry.set_acl_children_indices(vec![0, 3, 5]);
        indexedlog.put_entry(entry).unwrap();
        indexedlog.flush_log().unwrap();

        let mut store = TreeStore::empty();
        store.indexedlog_local = Some(indexedlog);

        let read_entry = store
            .get_indexedlog_entry_direct(&tree_id)
            .unwrap()
            .unwrap();
        assert_eq!(read_entry.acl_children_indices(), Some(&[0, 3, 5][..]));
    }

    #[test]
    fn test_insert_data_with_acl_children_indices() {
        let tempdir = TempDir::new().unwrap();
        let indexedlog = make_data_store(&tempdir);

        let mut store = TreeStore::empty();
        store.indexedlog_local = Some(indexedlog);

        let child_dir_id = HgId::from_hex(b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        let child_file_id = HgId::from_hex(b"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap();
        let tree_data = format!(
            "dir\0{}t\nfile\0{}\n",
            child_dir_id.to_hex(),
            child_file_id.to_hex(),
        );
        let tree_bytes: Bytes = Bytes::copy_from_slice(tree_data.as_bytes());

        let path = RepoPathBuf::from_string("root".to_string()).unwrap();
        let opts = InsertOpts {
            kind: Kind::Tree,
            acl_children_indices: Some(vec![0]),
            ..Default::default()
        };
        let id = store.insert_data(opts, &path, tree_bytes.into()).unwrap();

        let tree_entry = store.get_local_tree_direct(id).unwrap().unwrap();
        let acl = tree_entry.children_with_acls().unwrap();
        assert_eq!(acl.len(), 1);
        assert_eq!(acl[0].0.as_str(), "dir");
        assert_eq!(acl[0].1, child_dir_id);
    }
}
