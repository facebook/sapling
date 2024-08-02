/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Instant;

use anyhow::Result;
use crossbeam::channel::Sender;
use tracing::field;
use types::fetch_mode::FetchMode;
use types::hgid::NULL_ID;
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
                let aux_data = entry.children_aux_data();
                for (hgid, aux) in aux_data.into_iter() {
                    match aux {
                        AuxData::File(file_aux) => {
                            if let Some(aux_cache) = aux_cache.as_ref() {
                                tracing::trace!(?hgid, "writing to aux cache");
                                aux_cache.put(hgid, &file_aux)?;
                            }
                        }
                        AuxData::Tree(tree_aux) => {
                            if let Some(tree_aux_store) = tree_aux_store.as_ref() {
                                tracing::trace!(?hgid, "writing to tree aux store");
                                tree_aux_store.put(hgid, &tree_aux)?;
                            }
                        }
                    }
                }

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
}
