/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use flume::Sender;
use progress_model::ProgressBar;
use storemodel::FileAuxData;
use storemodel::TreeAuxData;
use tracing::field;
use types::FetchContext;
use types::HgId;
use types::Key;
use types::NodeInfo;
use types::hgid::NULL_ID;

use super::metrics::TREE_STORE_FETCH_METRICS;
use super::metrics::TREE_STORE_PREFETCH_METRICS;
use super::metrics::TreeStoreFetchMetrics;
use super::types::StoreTree;
use super::types::TreeAttributes;
use crate::AuxStore;
use crate::HgIdMutableHistoryStore;
use crate::IndexedLogHgIdDataStore;
use crate::IndexedLogHgIdHistoryStore;
use crate::SaplingRemoteApiTreeStore;
use crate::indexedlogdatastore::Entry;
use crate::indexedlogtreeauxstore::TreeAuxStore;
use crate::scmstore::KeyFetchError;
use crate::scmstore::fetch::CommonFetchState;
use crate::scmstore::fetch::FetchErrors;
use crate::scmstore::tree::types::AuxData;
use crate::scmstore::tree::types::LazyTree;

const FILE_AUX_BATCH_THRESHOLD: usize = 1000;
const TREE_AUX_BATCH_THRESHOLD: usize = 1000;
const TREE_BATCH_THRESHOLD: usize = 100;

pub struct FetchState {
    pub(crate) common: CommonFetchState<StoreTree>,

    /// Errors encountered during fetching.
    pub(crate) errors: FetchErrors,

    /// Track fetch metrics,
    pub(crate) metrics: &'static TreeStoreFetchMetrics,

    pub(crate) file_aux_cache: Option<Arc<AuxStore>>,
    pub(crate) tree_aux_cache: Option<Arc<TreeAuxStore>>,

    // Enqueue aux data so we can process it more efficiently all at once.
    pub(crate) file_aux_to_cache: Vec<(HgId, FileAuxData)>,
    pub(crate) tree_aux_to_cache: Vec<(HgId, TreeAuxData)>,

    pub(crate) tree_cache: Option<Arc<IndexedLogHgIdDataStore>>,
    pub(crate) trees_to_cache: Vec<(HgId, Entry)>,
}

impl Drop for FetchState {
    fn drop(&mut self) {
        self.flush_file_aux();
        self.flush_tree_aux();
        self.flush_trees();

        self.common.results(std::mem::take(&mut self.errors), false);
    }
}

impl FetchState {
    pub(crate) fn new(
        keys: impl IntoIterator<Item = Key>,
        attrs: TreeAttributes,
        found_tx: Sender<Result<(Key, StoreTree), KeyFetchError>>,
        fctx: FetchContext,
        bar: Arc<ProgressBar>,
        tree_cache: Option<Arc<IndexedLogHgIdDataStore>>,
        file_aux_cache: Option<Arc<AuxStore>>,
        tree_aux_cache: Option<Arc<TreeAuxStore>>,
    ) -> Self {
        let cause = fctx.cause();
        FetchState {
            common: CommonFetchState::new(keys, attrs, found_tx, fctx, bar),
            errors: FetchErrors::new(),
            metrics: if cause.is_prefetch() {
                &TREE_STORE_PREFETCH_METRICS
            } else {
                &TREE_STORE_FETCH_METRICS
            },
            file_aux_cache,
            file_aux_to_cache: Vec::new(),
            tree_aux_cache,
            tree_aux_to_cache: Vec::new(),
            tree_cache,
            trees_to_cache: Vec::new(),
        }
    }

    pub(crate) fn fetch_edenapi(
        &mut self,
        edenapi: &SaplingRemoteApiTreeStore,
        attributes: edenapi_types::TreeAttributes,
        indexedlog_cache: Option<&IndexedLogHgIdDataStore>,
        historystore_cache: Option<&IndexedLogHgIdHistoryStore>,
    ) -> Result<()> {
        let pending: Vec<_> = self
            .common
            .pending(
                TreeAttributes::CONTENT | TreeAttributes::PARENTS | TreeAttributes::AUX_DATA,
                false,
            )
            .map(|(key, _attrs)| key.clone())
            .collect();

        if pending.is_empty() {
            return Ok(());
        }

        let start_time = Instant::now();

        self.metrics.edenapi.fetch(pending.len());

        let span = tracing::info_span!(
            "fetch_edenapi",
            downloaded = field::Empty,
            uploaded = field::Empty,
            requests = field::Empty,
            time = field::Empty,
            latency = field::Empty,
            download_speed = field::Empty,
        );
        let _enter = span.enter();
        tracing::debug!(
            "attempt to fetch {} keys from edenapi ({:?})",
            pending.len(),
            edenapi.url()
        );

        let bar = ProgressBar::new_adhoc("SLAPI", pending.len() as u64, "trees");

        let response = edenapi
            .trees_blocking(self.common.fctx.clone(), pending, Some(attributes))
            .map_err(|e| e.tag_network())?;
        for entry in response.entries {
            bar.increase_position(1);

            let entry = entry?;
            let key = entry.key.clone();
            let entry = LazyTree::SaplingRemoteApi(entry);

            self.cache_child_aux_data(&entry);

            if self.tree_aux_cache.is_some() {
                if let Some(aux_data) = entry.aux_data() {
                    tracing::trace!(
                        hgid = %key.hgid,
                        "writing self to tree aux store"
                    );
                    self.tree_aux_to_cache.push((key.hgid, aux_data));
                    if self.tree_aux_to_cache.len() >= TREE_AUX_BATCH_THRESHOLD {
                        self.flush_tree_aux();
                    }
                }
            }

            if indexedlog_cache.is_some() {
                if let Some(entry) = entry.indexedlog_cache_entry(key.hgid)? {
                    self.trees_to_cache.push((entry.node(), entry));
                    if self.trees_to_cache.len() >= TREE_BATCH_THRESHOLD {
                        self.flush_trees();
                    }
                }
            }

            if let Some(historystore_cache) = &historystore_cache {
                if let Some(parents) = entry.parents() {
                    historystore_cache.add(
                        &key,
                        &NodeInfo {
                            parents: parents.to_keys(),
                            linknode: NULL_ID,
                        },
                    )?;
                }
            }

            self.common.found(key, entry.into());
        }

        crate::util::record_edenapi_stats(&span, &response.stats);

        let _ = self
            .metrics
            .edenapi
            .time_from_duration(start_time.elapsed());

        Ok(())
    }

    fn cache_child_aux_data(&mut self, tree: &LazyTree) {
        let aux_cache = &self.file_aux_cache;
        let tree_aux_store = &self.tree_aux_cache;

        if aux_cache.is_none() && tree_aux_store.is_none() {
            return;
        }

        for (hgid, aux) in tree.children_aux_data() {
            match aux {
                AuxData::File(file_aux) => {
                    self.file_aux_to_cache.push((hgid, file_aux));
                    if self.file_aux_to_cache.len() >= FILE_AUX_BATCH_THRESHOLD {
                        self.flush_file_aux();
                    }
                }
                AuxData::Tree(tree_aux) => {
                    self.tree_aux_to_cache.push((hgid, tree_aux));
                    if self.tree_aux_to_cache.len() >= TREE_AUX_BATCH_THRESHOLD {
                        self.flush_tree_aux();
                    }
                }
            }
        }
    }

    fn flush_file_aux(&mut self) {
        if let Some(aux_cache) = &self.file_aux_cache {
            if let Err(err) = aux_cache.put_batch(&mut self.file_aux_to_cache) {
                self.errors.other_error(err);
            }
            self.file_aux_to_cache.clear();
        }
    }

    fn flush_tree_aux(&mut self) {
        if let Some(tree_aux_cache) = &self.tree_aux_cache {
            if let Err(err) = tree_aux_cache.put_batch(&mut self.tree_aux_to_cache) {
                self.errors.other_error(err);
            }
            self.tree_aux_to_cache.clear();
        }
    }

    fn flush_trees(&mut self) {
        if let Some(tree_cache) = &self.tree_cache {
            if let Err(err) = tree_cache.put_batch(&mut self.trees_to_cache) {
                self.errors.other_error(err);
            }
            self.trees_to_cache.clear();
        }
    }
}
