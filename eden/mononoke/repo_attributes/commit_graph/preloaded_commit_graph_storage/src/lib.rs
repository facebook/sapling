/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use bytes::Bytes;
use cloned::cloned;
use commit_graph_thrift as thrift;
use commit_graph_types::edges::ChangesetEdges;
use commit_graph_types::edges::ChangesetNode;
use commit_graph_types::edges::CompactChangesetEdges;
use commit_graph_types::storage::CommitGraphStorage;
use commit_graph_types::storage::FetchedChangesetEdges;
use commit_graph_types::storage::Prefetch;
use context::CoreContext;
use fbthrift::compact_protocol;
use mononoke_types::ChangesetId;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::Generation;
use mononoke_types::RepositoryId;
use reloader::Loader;
use reloader::Reloader;
use slog::info;
use vec1::Vec1;

#[cfg(test)]
mod tests;

const DEFAULT_RELOADING_INTERVAL_SECS: u64 = 60 * 60;

/// A commit graph storage that wraps another storage and periodically preloads
/// the changeset edges of the commit graph from the blobstore. Writes are passed
/// to the underlying storage, while reads first search the preloaded changeset
/// edges and fall over to the underlying storage for the missing changesets.
///
/// Useful for commit graphs with complicated structure that are small enough
/// to fit into memory.
pub struct PreloadedCommitGraphStorage {
    repo_id: RepositoryId,
    preloaded_edges: Reloader<PreloadedEdges>,
    persistent_storage: Arc<dyn CommitGraphStorage>,
}

pub struct PreloadedEdgesLoader {
    ctx: CoreContext,
    blobstore_without_cache: Arc<dyn Blobstore>,
    blobstore_key: String,
}

#[derive(Debug, Default)]
pub struct PreloadedEdges {
    pub cs_id_to_edges: HashMap<ChangesetId, CompactChangesetEdges>,
    pub unique_id_to_cs_id: HashMap<NonZeroU32, ChangesetId>,
    pub max_sql_id: Option<u64>,
}

impl PreloadedEdges {
    pub fn to_thrift(&self) -> Result<thrift::PreloadedEdges> {
        Ok(thrift::PreloadedEdges {
            edges: self
                .unique_id_to_cs_id
                .iter()
                .map(|(unique_id, cs_id)| {
                    Ok(self
                        .cs_id_to_edges
                        .get(cs_id)
                        .ok_or_else(|| anyhow!("Missing changeset edges for {}", cs_id))?
                        .to_thrift(*cs_id, *unique_id))
                })
                .collect::<Result<_>>()?,
            max_sql_id: self.max_sql_id.map(|id| id as i64),
        })
    }

    pub fn from_thrift(preloaded_edges: thrift::PreloadedEdges) -> Result<Self> {
        let unique_id_to_cs_id = preloaded_edges
            .edges
            .iter()
            .map(|edges| {
                Ok((
                    NonZeroU32::new(edges.unique_id as u32)
                        .ok_or_else(|| anyhow!("Couldn't convert unique_id to NonZeroU32"))?,
                    ChangesetId::from_thrift(edges.cs_id.clone())?,
                ))
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            unique_id_to_cs_id,
            cs_id_to_edges: preloaded_edges
                .edges
                .into_iter()
                .map(|edges| {
                    Ok((
                        ChangesetId::from_thrift(edges.cs_id.clone())?,
                        CompactChangesetEdges::from_thrift(edges)?,
                    ))
                })
                .collect::<Result<_>>()?,
            max_sql_id: preloaded_edges.max_sql_id.map(|id| id as u64),
        })
    }

    fn get_node(&self, unique_id: NonZeroU32) -> Result<ChangesetNode> {
        let cs_id = *self
            .unique_id_to_cs_id
            .get(&unique_id)
            .ok_or_else(|| anyhow!("Missing changeset id for unique id: {}", unique_id))?;
        let edges = self
            .cs_id_to_edges
            .get(&cs_id)
            .ok_or_else(|| anyhow!("Missing changeset edges for {}", cs_id))?;

        Ok(ChangesetNode {
            cs_id,
            generation: Generation::new(edges.generation as u64),
            skip_tree_depth: edges.skip_tree_depth as u64,
            p1_linear_depth: edges.p1_linear_depth as u64,
        })
    }

    pub fn get(&self, cs_id: &ChangesetId) -> Result<Option<ChangesetEdges>> {
        let compact_edges = match self.cs_id_to_edges.get(cs_id) {
            Some(edges) => edges,
            None => return Ok(None),
        };

        Ok(Some(ChangesetEdges {
            node: ChangesetNode {
                cs_id: *cs_id,
                generation: Generation::new(compact_edges.generation as u64),
                skip_tree_depth: compact_edges.skip_tree_depth as u64,
                p1_linear_depth: compact_edges.p1_linear_depth as u64,
            },
            parents: compact_edges
                .parents
                .iter()
                .map(|parent_id| self.get_node(*parent_id))
                .collect::<Result<_>>()?,
            merge_ancestor: compact_edges
                .merge_ancestor
                .map(|merge_ancestor| self.get_node(merge_ancestor))
                .transpose()?,
            skip_tree_parent: compact_edges
                .skip_tree_parent
                .map(|skip_tree_parent| self.get_node(skip_tree_parent))
                .transpose()?,
            skip_tree_skew_ancestor: compact_edges
                .skip_tree_skew_ancestor
                .map(|skip_tree_skew_ancestor| self.get_node(skip_tree_skew_ancestor))
                .transpose()?,
            p1_linear_skew_ancestor: compact_edges
                .p1_linear_skew_ancestor
                .map(|p1_linear_skew_ancestor| self.get_node(p1_linear_skew_ancestor))
                .transpose()?,
        }))
    }
}

#[derive(Debug, Default)]
pub struct ExtendablePreloadedEdges {
    preloaded_edges: PreloadedEdges,
    cs_id_to_unique_id: HashMap<ChangesetId, NonZeroU32>,
}

impl ExtendablePreloadedEdges {
    pub fn from_preloaded_edges(preloaded_edges: PreloadedEdges) -> Self {
        let cs_id_to_unique_id = preloaded_edges
            .unique_id_to_cs_id
            .iter()
            .map(|(unique_id, cs_id)| (*cs_id, *unique_id))
            .collect();
        Self {
            preloaded_edges,
            cs_id_to_unique_id,
        }
    }

    pub fn into_preloaded_edges(self) -> PreloadedEdges {
        self.preloaded_edges
    }

    pub fn unique_id(&mut self, cs_id: ChangesetId) -> NonZeroU32 {
        match self.cs_id_to_unique_id.get(&cs_id) {
            Some(unique_id) => *unique_id,
            None => {
                let unique_id = NonZeroU32::new(self.cs_id_to_unique_id.len() as u32 + 1).unwrap();
                self.cs_id_to_unique_id.insert(cs_id, unique_id);
                self.preloaded_edges
                    .unique_id_to_cs_id
                    .insert(unique_id, cs_id);
                unique_id
            }
        }
    }

    pub fn add(&mut self, edges: ChangesetEdges) -> Result<()> {
        let _unique_id = self.unique_id(edges.node.cs_id);
        let parents = edges
            .parents
            .iter()
            .map(|parent| self.unique_id(parent.cs_id))
            .collect();
        let merge_ancestor = edges
            .merge_ancestor
            .map(|merge_ancestor| self.unique_id(merge_ancestor.cs_id));
        let skip_tree_parent = edges
            .skip_tree_parent
            .map(|skip_tree_parent| self.unique_id(skip_tree_parent.cs_id));
        let skip_tree_skew_ancestor = edges
            .skip_tree_skew_ancestor
            .map(|skip_tree_skew_ancestor| self.unique_id(skip_tree_skew_ancestor.cs_id));
        let p1_linear_skew_ancestor = edges
            .p1_linear_skew_ancestor
            .map(|p1_linear_skew_ancestor| self.unique_id(p1_linear_skew_ancestor.cs_id));

        match self.preloaded_edges.cs_id_to_edges.insert(
            edges.node.cs_id,
            CompactChangesetEdges {
                generation: edges.node.generation.value() as u32,
                skip_tree_depth: edges.node.skip_tree_depth as u32,
                p1_linear_depth: edges.node.p1_linear_depth as u32,
                parents,
                merge_ancestor,
                skip_tree_parent,
                skip_tree_skew_ancestor,
                p1_linear_skew_ancestor,
            },
        ) {
            Some(old_edges) => Err(anyhow!("Duplicate changeset edges found: {:?}", old_edges)),
            None => Ok(()),
        }
    }

    pub fn update_max_sql_id(&mut self, max_sql_id: u64) {
        self.preloaded_edges.max_sql_id = Some(max_sql_id);
    }
}

pub fn deserialize_preloaded_edges(bytes: Bytes) -> Result<PreloadedEdges> {
    let preloaded_edges: thrift::PreloadedEdges = compact_protocol::deserialize(bytes)?;

    PreloadedEdges::from_thrift(preloaded_edges)
}

#[async_trait]
impl Loader<PreloadedEdges> for PreloadedEdgesLoader {
    async fn load(&mut self) -> Result<Option<PreloadedEdges>> {
        tokio::task::spawn({
            cloned!(self.ctx, self.blobstore_without_cache, self.blobstore_key);
            async move {
                info!(ctx.logger(), "Started preloading commit graph");
                let maybe_bytes = blobstore_without_cache.get(&ctx, &blobstore_key).await?;
                match maybe_bytes {
                    Some(bytes) => {
                        let bytes = bytes.into_raw_bytes();
                        let preloaded_edges =
                            tokio::task::spawn_blocking(move || deserialize_preloaded_edges(bytes))
                                .await??;
                        info!(
                            ctx.logger(),
                            "Finished preloading commit graph ({} changesets)",
                            preloaded_edges.cs_id_to_edges.len()
                        );
                        Ok(Some(preloaded_edges))
                    }
                    None => Ok(Some(Default::default())),
                }
            }
        })
        .await?
    }
}

impl PreloadedCommitGraphStorage {
    pub async fn from_blobstore(
        ctx: &CoreContext,
        repo_id: RepositoryId,
        blobstore_without_cache: Arc<dyn Blobstore>,
        preloaded_edges_blobstore_key: String,
        persistent_storage: Arc<dyn CommitGraphStorage>,
    ) -> Result<Arc<Self>> {
        let loader = PreloadedEdgesLoader {
            ctx: ctx.clone(),
            blobstore_key: preloaded_edges_blobstore_key,
            blobstore_without_cache,
        };

        let reloader = Reloader::reload_periodically(
            ctx.clone(),
            move || {
                std::time::Duration::from_secs(
                    justknobs::get_as::<u64>(
                        "scm/mononoke:preloaded_commit_graph_reloading_interval_secs",
                        None,
                    )
                    .unwrap_or(DEFAULT_RELOADING_INTERVAL_SECS),
                )
            },
            loader,
        )
        .await?;
        Ok(Arc::new(Self {
            repo_id,
            preloaded_edges: reloader,
            persistent_storage,
        }))
    }
}

#[async_trait]
impl CommitGraphStorage for PreloadedCommitGraphStorage {
    fn repo_id(&self) -> RepositoryId {
        self.repo_id
    }

    async fn add(&self, ctx: &CoreContext, edges: ChangesetEdges) -> Result<bool> {
        self.persistent_storage.add(ctx, edges).await
    }

    async fn add_many(&self, ctx: &CoreContext, many_edges: Vec1<ChangesetEdges>) -> Result<usize> {
        self.persistent_storage.add_many(ctx, many_edges).await
    }

    async fn fetch_edges(&self, ctx: &CoreContext, cs_id: ChangesetId) -> Result<ChangesetEdges> {
        match self.preloaded_edges.load().get(&cs_id)? {
            Some(edges) => Ok(edges),
            None => self.persistent_storage.fetch_edges(ctx, cs_id).await,
        }
    }

    async fn maybe_fetch_edges(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Option<ChangesetEdges>> {
        match self.preloaded_edges.load().get(&cs_id)? {
            Some(edges) => Ok(Some(edges)),
            None => self.persistent_storage.maybe_fetch_edges(ctx, cs_id).await,
        }
    }

    async fn fetch_many_edges(
        &self,
        ctx: &CoreContext,
        cs_ids: &[ChangesetId],
        prefetch: Prefetch,
    ) -> Result<HashMap<ChangesetId, FetchedChangesetEdges>> {
        let edges = self.maybe_fetch_many_edges(ctx, cs_ids, prefetch).await?;
        if let Some(missing_changeset) = cs_ids.iter().find(|cs_id| !edges.contains_key(cs_id)) {
            Err(anyhow!(
                "Missing changeset from preloaded commit graph storage: {}",
                missing_changeset,
            ))
        } else {
            Ok(edges)
        }
    }

    async fn maybe_fetch_many_edges(
        &self,
        ctx: &CoreContext,
        cs_ids: &[ChangesetId],
        prefetch: Prefetch,
    ) -> Result<HashMap<ChangesetId, FetchedChangesetEdges>> {
        let preloaded_edges = self.preloaded_edges.load();
        let mut fetched_edges: HashMap<_, _> = cs_ids
            .iter()
            .filter_map(|cs_id| {
                preloaded_edges
                    .get(cs_id)
                    .map(|edges| edges.map(|edges| (*cs_id, edges.into())))
                    .transpose()
            })
            .collect::<Result<_>>()?;

        let unfetched_ids: Vec<_> = cs_ids
            .iter()
            .filter(|cs_id| !fetched_edges.contains_key(cs_id))
            .copied()
            .collect();

        if !unfetched_ids.is_empty() {
            fetched_edges.extend(
                self.persistent_storage
                    .maybe_fetch_many_edges(ctx, unfetched_ids.as_slice(), prefetch)
                    .await?,
            )
        }

        Ok(fetched_edges)
    }

    async fn find_by_prefix(
        &self,
        ctx: &CoreContext,
        cs_prefix: ChangesetIdPrefix,
        limit: usize,
    ) -> Result<ChangesetIdsResolvedFromPrefix> {
        self.persistent_storage
            .find_by_prefix(ctx, cs_prefix, limit)
            .await
    }

    async fn fetch_children(&self, ctx: &CoreContext, cs: ChangesetId) -> Result<Vec<ChangesetId>> {
        self.persistent_storage.fetch_children(ctx, cs).await
    }
}
