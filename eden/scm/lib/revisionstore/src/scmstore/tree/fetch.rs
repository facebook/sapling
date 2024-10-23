/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::future;
use std::time::Instant;

use anyhow::Result;
use async_runtime::block_on;
use cas_client::CasClient;
use crossbeam::channel::Sender;
use futures::StreamExt;
use tracing::field;
use types::fetch_mode::FetchMode;
use types::hgid::NULL_ID;
use types::AugmentedTree;
use types::AugmentedTreeWithDigest;
use types::CasDigest;
use types::CasDigestType;
use types::CasFetchedStats;
use types::Key;
use types::NodeInfo;

use super::metrics::TreeStoreFetchMetrics;
use super::types::StoreTree;
use super::types::TreeAttributes;
use crate::indexedlogtreeauxstore::TreeAuxStore;
use crate::scmstore::fetch::CommonFetchState;
use crate::scmstore::fetch::FetchErrors;
use crate::scmstore::tree::types::AuxData;
use crate::scmstore::tree::types::LazyTree;
use crate::scmstore::KeyFetchError;
use crate::AuxStore;
use crate::HgIdMutableHistoryStore;
use crate::IndexedLogHgIdDataStore;
use crate::IndexedLogHgIdHistoryStore;
use crate::SaplingRemoteApiTreeStore;

pub struct FetchState {
    pub(crate) common: CommonFetchState<StoreTree>,

    /// Errors encountered during fetching.
    pub(crate) errors: FetchErrors,

    /// Track fetch metrics,
    pub(crate) metrics: TreeStoreFetchMetrics,
}

impl FetchState {
    pub(crate) fn new(
        keys: impl IntoIterator<Item = Key>,
        attrs: TreeAttributes,
        found_tx: Sender<Result<(Key, StoreTree), KeyFetchError>>,
        fetch_mode: FetchMode,
    ) -> Self {
        FetchState {
            common: CommonFetchState::new(keys, attrs, found_tx, fetch_mode),
            errors: FetchErrors::new(),
            metrics: TreeStoreFetchMetrics::default(),
        }
    }

    pub(crate) fn fetch_edenapi(
        &mut self,
        edenapi: &SaplingRemoteApiTreeStore,
        attributes: edenapi_types::TreeAttributes,
        indexedlog_cache: Option<&IndexedLogHgIdDataStore>,
        aux_cache: Option<&AuxStore>,
        tree_aux_store: Option<&TreeAuxStore>,
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

        let response = edenapi
            .trees_blocking(pending, Some(attributes))
            .map_err(|e| e.tag_network())?;
        for entry in response.entries {
            let entry = entry?;
            let key = entry.key.clone();
            let entry = LazyTree::SaplingRemoteApi(entry);

            if aux_cache.is_some() || tree_aux_store.is_some() {
                cache_child_aux_data(&entry, aux_cache, tree_aux_store)?;

                if let Some(aux_data) = entry.aux_data() {
                    if let Some(tree_aux_store) = tree_aux_store.as_ref() {
                        tracing::trace!(
                            hgid = %key.hgid,
                            "writing self to tree aux store"
                        );
                        tree_aux_store.put(key.hgid, &aux_data)?;
                    }
                }
            }

            if let Some(indexedlog_cache) = &indexedlog_cache {
                if let Some(entry) = entry.indexedlog_cache_entry(key.clone())? {
                    indexedlog_cache.put_entry(entry)?;
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

    pub(crate) fn fetch_cas(
        &mut self,
        cas_client: &dyn CasClient,
        aux_cache: Option<&AuxStore>,
        tree_aux_store: Option<&TreeAuxStore>,
    ) {
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

        let digest_with_keys: Vec<(CasDigest, Key)> = self
            .common
            .pending(TreeAttributes::CONTENT | TreeAttributes::PARENTS, false)
            .filter_map(|(key, store_tree)| {
                let aux_data = match store_tree.aux_data.as_ref() {
                    Some(aux_data) => {
                        tracing::trace!(target: "cas", ?key, ?aux_data, "found aux data for tree digest");
                        aux_data
                    }
                    None => {
                        tracing::trace!(target: "cas", ?key, "no aux data for tree digest");
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

        // Include the duplicates in the count.
        span.record("keys", digest_with_keys.len());

        let mut digest_to_key: HashMap<CasDigest, Vec<Key>> = HashMap::default();

        for (digest, key) in digest_with_keys {
            digest_to_key.entry(digest).or_default().push(key);
        }

        if digest_to_key.is_empty() {
            return;
        }

        let digests: Vec<CasDigest> = digest_to_key.keys().cloned().collect();

        let mut found = 0;
        let mut error = 0;
        let mut reqs = 0;

        let start_time = Instant::now();
        let mut total_stats = CasFetchedStats::default();

        block_on(async {
            cas_client.fetch(&digests, CasDigestType::Tree).await.for_each(|results| match results {
                Ok((stats, results)) => {
                    reqs += 1;
                    total_stats.add(&stats);
                    for (digest, data) in results {
                        let Some(mut keys) = digest_to_key.remove(&digest) else {
                            tracing::error!("got CAS result for unrequested digest {:?}", digest);
                            continue;
                        };

                        match data {
                            Err(err) => {
                                tracing::error!(?err, ?keys, ?digest, "CAS fetch error");
                                tracing::error!(target: "cas", ?err, ?keys, ?digest, "tree fetch error");
                                error += keys.len();
                                self.errors.multiple_keyed_error(keys, "CAS fetch error", err);
                            }
                            Ok(None) => {
                                tracing::trace!(target: "cas", ?keys, ?digest, "tree not in cas");
                                // miss
                            }
                            Ok(Some(data)) => match AugmentedTree::try_deserialize(&*data) {
                                Ok(tree) => {
                                    found += keys.len();
                                    tracing::trace!(target: "cas", ?keys, ?digest, "tree found in cas");

                                    let lazy_tree = LazyTree::Cas(AugmentedTreeWithDigest {
                                        augmented_manifest_id: digest.hash,
                                        augmented_manifest_size: digest.size,
                                        augmented_tree: tree,
                                    });

                                    if let Err(err) =
                                        cache_child_aux_data(&lazy_tree, aux_cache, tree_aux_store)
                                    {
                                        self.errors.multiple_keyed_error(keys, "cache child aux data failed", err);
                                    } else {
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
                                }
                                Err(err) => {
                                    error += keys.len();
                                    tracing::error!(target: "cas", ?err, ?keys, ?digest, "error deserializing tree");
                                    self.errors.multiple_keyed_error(keys, "CAS tree deserialization failed", err);
                                }
                            },
                        }
                    }
                    future::ready(())
                }
                Err(err) => {
                    tracing::error!(?err, "overall CAS error");
                    tracing::error!(target: "cas", ?err, "CAS error fetching trees");

                    // Don't propagate CAS error - we want to fall back to SLAPI.
                    reqs += 1;
                    error += 1;
                    future::ready(())
                }
            }).await;
        });

        span.record("hits", found);
        span.record("requests", reqs);
        span.record("time", start_time.elapsed().as_millis() as u64);

        let _ = self.metrics.cas.time_from_duration(start_time.elapsed());
        self.metrics.cas.fetch(digests.len());
        self.metrics.cas.err(error);
        self.metrics.cas.hit(found);
        self.metrics
            .cas_backend
            .zdb_bytes(total_stats.total_bytes_zdb);
        self.metrics
            .cas_backend
            .zgw_bytes(total_stats.total_bytes_zgw);
        self.metrics
            .cas_backend
            .manifold_bytes(total_stats.total_bytes_manifold);
        self.metrics
            .cas_backend
            .hedwig_bytes(total_stats.total_bytes_hedwig);
        self.metrics
            .cas_backend
            .zdb_queries(total_stats.queries_zdb);
        self.metrics
            .cas_backend
            .zgw_queries(total_stats.queries_zgw);
        self.metrics
            .cas_backend
            .manifold_queries(total_stats.queries_manifold);
        self.metrics
            .cas_backend
            .hedwig_queries(total_stats.queries_hedwig);
    }
}

fn cache_child_aux_data(
    tree: &LazyTree,
    aux_cache: Option<&AuxStore>,
    tree_aux_store: Option<&TreeAuxStore>,
) -> Result<()> {
    if aux_cache.is_none() && tree_aux_store.is_none() {
        return Ok(());
    }

    let aux_data = tree.children_aux_data();
    for (hgid, aux) in aux_data.into_iter() {
        match aux {
            AuxData::File(file_aux) => {
                if let Some(aux_cache) = aux_cache.as_ref() {
                    if !aux_cache.contains(hgid)? {
                        tracing::trace!(?hgid, "writing to aux cache");
                        aux_cache.put(hgid, &file_aux)?;
                    }
                }
            }
            AuxData::Tree(tree_aux) => {
                if let Some(tree_aux_store) = tree_aux_store.as_ref() {
                    if !tree_aux_store.contains(hgid)? {
                        tracing::trace!(?hgid, "writing to tree aux store");
                        tree_aux_store.put(hgid, &tree_aux)?;
                    }
                }
            }
        }
    }
    Ok(())
}
