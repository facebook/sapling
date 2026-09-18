/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use async_runtime::stream_to_iter;
use blob::Blob;
use cas_client::CasDigest;
use cas_client::CasDigestType;
use cas_client::CasFetchManager;
use cas_client::CasFetchOutcome;
use edenapi::Response;
use edenapi_types::TreeEntry;
use futures::StreamExt;
use manifest_augmented_tree::AugmentedTree;
use manifest_augmented_tree::AugmentedTreeWithDigest;
use progress_model::ProgressBar;
use smallvec::SmallVec;
use storemodel::FileAuxData;
use storemodel::SerializationFormat;
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
use crate::scmstore::fetch::CommonFetchState;
use crate::scmstore::fetch::FetchErrors;
use crate::scmstore::fetch::FetchItemsWriter;
use crate::scmstore::fetch::MaxFetchCount;
use crate::scmstore::tree::types::AuxData;
use crate::scmstore::tree::types::LazyTree;

const FILE_AUX_BATCH_THRESHOLD: usize = 1000;
const TREE_AUX_BATCH_THRESHOLD: usize = 1000;
const TREE_BATCH_THRESHOLD: usize = 100;

fn cas_tree_entry(content: Blob, digest: CasDigest) -> Result<TreeEntry> {
    let augmented_tree = match content {
        Blob::Bytes(bytes) => AugmentedTree::try_deserialize(bytes.as_ref()),
        #[cfg(fbcode_build)]
        Blob::IOBuf(buf) => AugmentedTree::try_deserialize(buf.cursor()),
    }
    .context("deserializing CAS augmented tree")?;
    TreeEntry::try_from(AugmentedTreeWithDigest {
        augmented_manifest_id: digest.hash,
        augmented_manifest_size: digest.size,
        augmented_tree,
    })
    .context("converting CAS augmented tree")
}

pub struct FetchState<'a> {
    pub(crate) common: CommonFetchState<'a, StoreTree>,

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

impl Drop for FetchState<'_> {
    fn drop(&mut self) {
        self.flush_file_aux();
        self.flush_tree_aux();
        self.flush_trees();

        self.common.results(std::mem::take(&mut self.errors), false);
    }
}

impl<'a> FetchState<'a> {
    pub(crate) fn new(
        keys: impl IntoIterator<Item = Key>,
        attrs: TreeAttributes,
        results: &'a mut FetchItemsWriter<StoreTree>,
        fctx: FetchContext,
        bar: Arc<ProgressBar>,
        tree_cache: Option<Arc<IndexedLogHgIdDataStore>>,
        file_aux_cache: Option<Arc<AuxStore>>,
        tree_aux_cache: Option<Arc<TreeAuxStore>>,
        max_fetch_count: MaxFetchCount,
    ) -> Self {
        let cause = fctx.cause();
        FetchState {
            common: CommonFetchState::new(keys, attrs, results, fctx, bar, max_fetch_count),
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
        historystore_cache: Option<&IndexedLogHgIdHistoryStore>,
        verify_hash: bool,
        format: SerializationFormat,
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

        let Response { entries, stats } = async_runtime::block_on(edenapi.trees(
            self.common.fctx.clone(),
            pending,
            Some(attributes),
        ))
        .map_err(|e| e.tag_network())?;

        for entry in stream_to_iter(entries) {
            let entry = entry.map_err(|e| e.tag_network())?;
            bar.increase_position(1);

            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    if let Some(key) = err.key.clone() {
                        self.errors.keyed_error(key, err.into());
                        continue;
                    }
                    return Err(err.into());
                }
            };
            self.accept_tree_entry(entry, historystore_cache, verify_hash, format)?;
        }

        match async_runtime::block_on(stats) {
            Ok(stats) => crate::util::record_edenapi_stats(&span, &stats),
            Err(err) => tracing::debug!(
                error = ?err,
                "failed to collect SaplingRemoteAPI response stats"
            ),
        }

        let _ = self
            .metrics
            .edenapi
            .time_from_duration(start_time.elapsed());

        Ok(())
    }

    pub(crate) fn fetch_cas(
        &mut self,
        cas_manager: &CasFetchManager,
        historystore_cache: Option<&IndexedLogHgIdHistoryStore>,
        verify_hash: bool,
        format: SerializationFormat,
    ) {
        if self.common.request_attrs == TreeAttributes::AUX_DATA {
            // If we are only requesting aux data, don't bother querying CAS. Aux data is
            // required to query CAS, so CAS cannot possibly help.
            return;
        }

        let digest_to_keys = self
            .common
            .pending(TreeAttributes::CONTENT, false)
            .filter_map(|(key, tree)| {
                let aux_data = tree.aux_data.as_ref()?;
                if !cas_manager.can_fetch_blob(aux_data.augmented_manifest_size) {
                    tracing::trace!(
                        target: "cas_client",
                        ?key,
                        size = aux_data.augmented_manifest_size,
                        "skipping CAS fetch for oversized tree"
                    );
                    return None;
                }

                Some((
                    CasDigest {
                        hash: aux_data.augmented_manifest_id,
                        size: aux_data.augmented_manifest_size,
                    },
                    key.clone(),
                ))
            })
            .fold(
                HashMap::<CasDigest, SmallVec<[Key; 1]>>::new(),
                |mut by_digest, (digest, key)| {
                    by_digest.entry(digest).or_default().push(key);
                    by_digest
                },
            );
        if digest_to_keys.is_empty() {
            return;
        }

        let keys_fetch_count = digest_to_keys.values().map(|keys| keys.len()).sum();
        let digests: Vec<_> = digest_to_keys.keys().copied().collect();
        let Some((guard, mut batches)) = cas_manager.fetch(&digests, CasDigestType::Tree) else {
            tracing::debug!(
                target: "cas_client",
                keys = keys_fetch_count,
                "skipping CAS tree fetch"
            );
            return;
        };
        self.common.fctx.set_fetch_from_cas_attempted(true);

        let mut pending = digest_to_keys;
        let mut hits = 0;
        let mut errors = 0;
        let mut requests = 0;
        let mut healthy = true;
        let start = Instant::now();
        let bar = ProgressBar::new_adhoc("CAS", digests.len() as u64, "trees");

        while let Some(batch) = async_runtime::block_on(batches.next()) {
            requests += 1;
            let batch = match batch {
                Ok(batch) => batch,
                Err(error) => {
                    healthy = false;
                    errors += 1;
                    tracing::warn!(
                        target: "cas_client",
                        ?error,
                        "CAS tree batch fetch failed"
                    );
                    continue;
                }
            };
            self.metrics.cas_backend.update(&batch.backend_stats);

            for (digest, result) in batch.results {
                bar.increase_position(1);
                let Some(keys) = pending.remove(&digest) else {
                    healthy = false;
                    tracing::error!(
                        target: "cas_client",
                        ?digest,
                        "CAS returned an unrequested tree digest"
                    );
                    continue;
                };

                let content = match result {
                    Ok(Some(content)) => content,
                    Ok(None) => continue,
                    Err(error) => {
                        healthy = false;
                        let key_count = keys.len();
                        errors += key_count;
                        tracing::warn!(
                            target: "cas_client",
                            ?digest,
                            ?error,
                            key_count,
                            "CAS tree digest fetch failed"
                        );
                        self.errors.multiple_keyed_error(
                            keys,
                            format!("CAS tree fetch failed for {digest:?}"),
                            error,
                        );
                        continue;
                    }
                };

                let mut entry = match cas_tree_entry(content, digest) {
                    Ok(entry) => entry,
                    Err(error) => {
                        healthy = false;
                        let key_count = keys.len();
                        errors += key_count;
                        tracing::warn!(
                            target: "cas_client",
                            ?digest,
                            ?error,
                            key_count,
                            "failed to decode CAS tree; recording error for EdenAPI fallback"
                        );
                        self.errors.multiple_keyed_error(
                            keys,
                            format!("invalid CAS tree for {digest:?}"),
                            error,
                        );
                        continue;
                    }
                };

                let mut keys = keys;
                keys.retain(|key| {
                    let matches = entry.key.hgid == key.hgid;
                    if !matches {
                        healthy = false;
                        errors += 1;
                        tracing::warn!(
                            target: "cas_client",
                            ?digest,
                            expected = %key.hgid,
                            actual = %entry.key.hgid,
                            "CAS tree node does not match the requested tree"
                        );
                        self.errors.keyed_error(
                            key.clone(),
                            anyhow!(
                                "CAS tree node mismatch for {digest:?}: expected {}, got {}",
                                key.hgid,
                                entry.key.hgid
                            ),
                        );
                    }
                    matches
                });

                let Some(last_key) = keys.pop() else {
                    continue;
                };

                let tree_hgid = entry.key.hgid;
                // Set the requested path so non-root CAS trees are strictly hash-verified;
                // an empty path permits legacy root-manifest hash mismatches.
                entry.key.path = last_key.path.clone();
                let tree = LazyTree::SaplingRemoteApi(entry, verify_hash, format);

                // Tree content is indexed by Hg ID, and every key above was checked against
                // `tree_hgid`, so one cache entry serves every repository path.
                if let Err(error) = self.cache_tree_content(tree_hgid, &tree) {
                    healthy = false;
                    let key_count = keys.len() + 1;
                    errors += key_count;
                    tracing::warn!(
                        target: "cas_client",
                        ?digest,
                        ?error,
                        key_count,
                        "failed to cache CAS tree"
                    );
                    self.errors.multiple_keyed_error(
                        keys.into_iter().chain([last_key]),
                        format!("failed to cache CAS tree for {digest:?}"),
                        error,
                    );
                    continue;
                }
                self.cache_child_aux_data(&tree);

                let mut accept_key = |key: Key, tree: LazyTree| {
                    let tree_for_key = cas_tree_for_key(tree, &key);
                    match self.accept_tree_for_key(
                        key.clone(),
                        tree_for_key,
                        None,
                        historystore_cache,
                    ) {
                        Ok(true) => hits += 1,
                        Ok(false) => {}
                        Err(error) => {
                            errors += 1;
                            self.errors.keyed_error(key, error);
                        }
                    }
                };
                for key in keys {
                    accept_key(key, tree.clone());
                }
                accept_key(last_key, tree);
            }
        }

        guard.finish(if healthy {
            CasFetchOutcome::Healthy
        } else {
            CasFetchOutcome::Failed
        });

        let elapsed = start.elapsed();
        self.metrics.cas.fetch(keys_fetch_count);
        self.metrics.cas.hit(hits);
        self.metrics.cas.err(errors);
        self.metrics.cas.miss(keys_fetch_count - hits);
        self.metrics.cas.time_from_duration(elapsed).ok();
        tracing::debug!(
            target: "cas_client",
            keys = keys_fetch_count,
            hits,
            errors,
            requests,
            duration = ?elapsed,
            "CAS tree fetch completed"
        );
    }

    fn accept_tree_entry(
        &mut self,
        entry: TreeEntry,
        historystore_cache: Option<&IndexedLogHgIdHistoryStore>,
        verify_hash: bool,
        format: SerializationFormat,
    ) -> Result<bool> {
        let key = entry.key.clone();
        let entry = LazyTree::SaplingRemoteApi(entry, verify_hash, format);
        self.cache_child_aux_data(&entry);
        let aux_data = entry.aux_data()?;

        if self.tree_aux_cache.is_some()
            && let Some(aux_data) = aux_data.as_ref()
        {
            tracing::trace!(
                hgid = %key.hgid,
                "writing self to tree aux store"
            );
            self.tree_aux_to_cache.push((key.hgid, aux_data.clone()));
            if self.tree_aux_to_cache.len() >= TREE_AUX_BATCH_THRESHOLD {
                self.flush_tree_aux();
            }
        }

        if let Err(error) = self.cache_tree_content(key.hgid, &entry) {
            self.errors.keyed_error(key, error);
            return Ok(false);
        }

        self.accept_tree_for_key(key, entry, aux_data, historystore_cache)
    }

    fn cache_tree_content(&mut self, hgid: HgId, entry: &LazyTree) -> Result<()> {
        if self.tree_cache.is_none() {
            return Ok(());
        }

        let Some(cache_entry) = entry.indexedlog_cache_entry(hgid)? else {
            return Ok(());
        };
        self.trees_to_cache.push((cache_entry.node(), cache_entry));
        if self.trees_to_cache.len() >= TREE_BATCH_THRESHOLD {
            self.flush_trees();
        }

        Ok(())
    }

    fn accept_tree_for_key(
        &mut self,
        key: Key,
        entry: LazyTree,
        aux_data: Option<TreeAuxData>,
        historystore_cache: Option<&IndexedLogHgIdHistoryStore>,
    ) -> Result<bool> {
        if let Some(historystore_cache) = historystore_cache {
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

        let parents = entry.parents();
        let entry = if self.common.fctx.mode().ignore_result() {
            LazyTree::Null
        } else {
            entry
        };
        Ok(self.common.found(
            key,
            StoreTree {
                parents,
                content: Some(entry),
                aux_data,
            },
        ))
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

fn cas_tree_for_key(tree: LazyTree, key: &Key) -> LazyTree {
    match tree {
        LazyTree::SaplingRemoteApi(mut entry, verify_hash, format) => {
            entry.key = key.clone();
            LazyTree::SaplingRemoteApi(entry, verify_hash, format)
        }
        tree => tree,
    }
}
