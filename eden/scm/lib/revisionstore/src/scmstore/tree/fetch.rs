/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::future;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use async_runtime::block_on;
use blob::Blob;
use cas_client::CasClient;
use flume::Sender;
use futures::StreamExt;
use manifest_augmented_tree::AugmentedTree;
use manifest_augmented_tree::AugmentedTreeWithDigest;
use progress_model::ProgressBar;
use storemodel::FileAuxData;
use storemodel::TreeAuxData;
use tracing::field;
use types::CasDigest;
use types::CasDigestType;
use types::CasFetchedStats;
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
        if let Some(aux_cache) = &self.file_aux_cache {
            if let Err(err) = aux_cache.put_batch(std::mem::take(&mut self.file_aux_to_cache)) {
                self.errors.other_error(err);
            }
        }

        if let Some(tree_aux_cache) = &self.tree_aux_cache {
            if let Err(err) = tree_aux_cache.put_batch(std::mem::take(&mut self.tree_aux_to_cache))
            {
                self.errors.other_error(err);
            }
        }

        if let Some(tree_cache) = &self.tree_cache {
            if let Err(err) = tree_cache.put_batch(std::mem::take(&mut self.trees_to_cache)) {
                self.errors.other_error(err);
            }
        }

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
                }
            }

            if indexedlog_cache.is_some() {
                if let Some(entry) = entry.indexedlog_cache_entry(key.hgid)? {
                    self.trees_to_cache.push((key.hgid, entry));
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

    pub(crate) fn fetch_cas(&mut self, cas_client: &dyn CasClient) {
        self.common.fctx.set_fetch_from_cas_attempted(true);

        if self.common.request_attrs == TreeAttributes::AUX_DATA {
            // If we are only requesting aux data, don't bother querying CAS. Aux data is
            // required to query CAS, so CAS cannot possibly help.
            return;
        }

        let span = tracing::info_span!(
            "fetch_cas",
            keys = field::Empty,
            hits = field::Empty,
            requests = field::Empty,
            time = field::Empty,
        );
        let _enter = span.enter();

        let bar = ProgressBar::new_adhoc("CAS", 0, "digests");

        let digest_with_keys: Vec<(CasDigest, Key)> = self
            .common
            .pending(TreeAttributes::CONTENT | TreeAttributes::PARENTS, false)
            .filter_map(|(key, store_tree)| {
                bar.increase_position(1);

                let aux_data = match store_tree.aux_data() {
                    Some(aux_data) => {
                        tracing::trace!(target: "cas_client", ?key, ?aux_data, "found aux data for tree digest");
                        aux_data
                    }
                    None => {
                        tracing::trace!(target: "cas_client", ?key, "no aux data for tree digest");
                        return None;
                    }
                };

                Some((
                    CasDigest {
                        hash: aux_data.augmented_manifest_id,
                        size: aux_data.augmented_manifest_size,
                    },
                    key.clone(),
                ))
            })
            .collect();

        drop(bar);

        // Include the duplicates in the count.
        let keys_fetch_count = digest_with_keys.len();

        span.record("keys", keys_fetch_count);

        let mut digest_to_key: HashMap<CasDigest, Vec<Key>> = HashMap::default();

        for (digest, key) in digest_with_keys {
            digest_to_key.entry(digest).or_default().push(key);
        }

        if digest_to_key.is_empty() {
            return;
        }

        let digests: Vec<CasDigest> = digest_to_key.keys().cloned().collect();

        let mut keys_found_count = 0;
        let mut error = 0;
        let mut reqs = 0;

        let start_time = Instant::now();
        let mut total_stats = CasFetchedStats::default();

        let bar = ProgressBar::new_adhoc("CAS", digests.len() as u64, "trees");

        async_runtime::block_in_place(|| {
            block_on(async {
                cas_client.fetch(self.common.fctx.clone(), &digests, CasDigestType::Tree).await.for_each(|results| {
                    match results {
                    Ok((stats, results)) => {
                        reqs += 1;
                        total_stats.add(&stats);
                        for (digest, data) in results {
                            bar.increase_position(1);

                            let Some(mut keys) = digest_to_key.remove(&digest) else {
                                tracing::error!("got CAS result for unrequested digest {:?}", digest);
                                continue;
                            };

                            match data {
                                Err(err) => {
                                    tracing::error!(?err, ?keys, ?digest, "CAS fetch error");
                                    tracing::error!(target: "cas_client", ?err, ?keys, ?digest, "tree fetch error");
                                    error += keys.len();
                                    self.errors.multiple_keyed_error(keys, "CAS fetch error", err);
                                }
                                Ok(None) => {
                                    tracing::trace!(target: "cas_client", ?keys, ?digest, "tree not in cas");
                                    // miss
                                }
                                Ok(Some(data)) => {
                                    let deserialization_result = match data {
                                        Blob::Bytes(bytes) => AugmentedTree::try_deserialize(bytes.as_ref()),
                                        #[allow(unexpected_cfgs)]
                                        #[cfg(fbcode_build)]
                                        Blob::IOBuf(buf) => AugmentedTree::try_deserialize(buf.cursor()),
                                    };
                                    match deserialization_result {
                                        Ok(tree) => {
                                            keys_found_count += keys.len();
                                            tracing::trace!(target: "cas_client", ?keys, ?digest, "tree found in cas");

                                            let lazy_tree = LazyTree::Cas(AugmentedTreeWithDigest {
                                                augmented_manifest_id: digest.hash,
                                                augmented_manifest_size: digest.size,
                                                augmented_tree: tree,
                                            });

                                            self.cache_child_aux_data(
                                                &lazy_tree,
                                            );
                                            if !keys.is_empty() {
                                                let last = keys.pop().unwrap();
                                                for key in keys {
                                                    self.common.found(
                                                        key,
                                                        StoreTree {
                                                            content: Some(lazy_tree.clone()),
                                                            parents: None,
                                                            aux_data: None,
                                                        },
                                                    );
                                                }
                                                // no clones needed
                                                self.common.found(
                                                    last,
                                                    StoreTree {
                                                        content: Some(lazy_tree),
                                                        parents: None,
                                                        aux_data: None,
                                                    },
                                                );
                                            }
                                        }
                                        Err(err) => {
                                            error += keys.len();
                                            tracing::error!(target: "cas_client", ?err, ?keys, ?digest, "error deserializing tree");
                                            self.errors.multiple_keyed_error(keys, "CAS tree deserialization failed", err);
                                        }
                                    }
                                }
                            }
                        }
                        future::ready(())
                    }
                    Err(err) => {
                        tracing::error!(?err, "overall CAS error");
                        tracing::error!(target: "cas_client", ?err, "CAS error fetching trees");

                        // Don't propagate CAS error - we want to fall back to SLAPI.
                        reqs += 1;
                        error += 1;
                        future::ready(())
                    }
                }}).await;
            })
        });

        span.record("hits", keys_found_count);
        span.record("requests", reqs);
        span.record("time", start_time.elapsed().as_millis() as u64);

        let _ = self.metrics.cas.time_from_duration(start_time.elapsed());
        self.metrics.cas.fetch(keys_fetch_count);
        self.metrics.cas.err(error);
        self.metrics.cas.hit(keys_found_count);
        self.metrics.cas.miss(keys_fetch_count - keys_found_count);
        self.metrics.cas_backend.update(&total_stats);
        self.metrics.cas_local_cache.update(&total_stats);
    }

    fn cache_child_aux_data(&mut self, tree: &LazyTree) {
        let aux_cache = &self.file_aux_cache;
        let tree_aux_store = &self.tree_aux_cache;

        if aux_cache.is_none() && tree_aux_store.is_none() {
            return;
        }

        for (hgid, aux) in tree.children_aux_data() {
            match aux {
                AuxData::File(file_aux) => self.file_aux_to_cache.push((hgid, file_aux)),
                AuxData::Tree(tree_aux) => self.tree_aux_to_cache.push((hgid, tree_aux)),
            }
        }
    }
}
