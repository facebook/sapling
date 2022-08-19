/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Commit Graph
//!
//! The graph of all commits in the repository.

#![feature(map_first_last)]

use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use context::CoreContext;
use maplit::hashset;
use mononoke_types::ChangesetId;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::Generation;
use smallvec::SmallVec;

use crate::edges::ChangesetEdges;
use crate::edges::ChangesetFrontier;
use crate::edges::ChangesetNode;
use crate::edges::ChangesetNodeParents;
use crate::edges::MergeAncestorOrSkipTreeParent;
use crate::storage::CommitGraphStorage;

pub mod edges;
pub mod storage;
#[cfg(test)]
mod tests;

/// The parents of a changeset.
///
/// This uses a smallvec, as there is usually exactly one.
pub type ChangesetParents = SmallVec<[ChangesetId; 1]>;

/// Commit Graph.
///
/// This contains the graph of all commits known to Mononoke for a particular
/// repository.  It provides methods for traversing the commit graph and
/// finding out graph-related information for the changesets contained
/// therein.
#[facet::facet]
pub struct CommitGraph {
    /// The storage back-end where the commits are actually stored.
    storage: Arc<dyn CommitGraphStorage>,
}

impl CommitGraph {
    pub fn new(storage: Arc<dyn CommitGraphStorage>) -> CommitGraph {
        CommitGraph { storage }
    }

    /// Add a new changeset to the commit graph.
    ///
    /// Returns true if a new changeset was inserted, or false if the
    /// changeset already existed.
    pub async fn add(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
        parents: ChangesetParents,
    ) -> Result<bool> {
        let parent_edges = self.storage.fetch_many_edges(ctx, &parents, None).await?;
        let mut max_parent_gen = 0;
        let mut edge_parents = ChangesetNodeParents::new();
        let mut merge_ancestor_or_skip_tree_parent = Default::default();
        for parent in &parents {
            let parent_edge = parent_edges
                .get(parent)
                .ok_or_else(|| anyhow!("Missing parent: {}", parent))?;
            max_parent_gen = max_parent_gen.max(parent_edge.node.generation.value());
            edge_parents.push(parent_edge.node);
            if parents.len() == 1 {
                merge_ancestor_or_skip_tree_parent = MergeAncestorOrSkipTreeParent::MergeAncestor(
                    parent_edge
                        .merge_ancestor_or_skip_tree_parent
                        .merge_ancestor()
                        .unwrap_or(parent_edge.node),
                );
            }
        }
        let generation = Generation::new(max_parent_gen + 1);
        // TODO(mbthomas): fill in the other ancestor edges.
        let edges = ChangesetEdges {
            node: ChangesetNode { cs_id, generation },
            parents: edge_parents,
            merge_ancestor_or_skip_tree_parent,
            skip_tree_skew_ancestor: None,
            p1_linear_skew_ancestor: None,
        };

        self.storage.add(ctx, edges).await
    }

    /// Find all changeset ids with a given prefix.
    pub async fn find_by_prefix(
        &self,
        ctx: &CoreContext,
        cs_prefix: ChangesetIdPrefix,
        limit: usize,
    ) -> Result<ChangesetIdsResolvedFromPrefix> {
        self.storage.find_by_prefix(ctx, cs_prefix, limit).await
    }

    /// Returns true if the changeset exists.
    pub async fn exists(&self, ctx: &CoreContext, cs_id: ChangesetId) -> Result<bool> {
        let edges = self.storage.fetch_edges(ctx, cs_id).await?;
        Ok(edges.is_some())
    }

    /// Returns the parents of a single changeset.
    pub async fn changeset_parents(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Option<ChangesetParents>> {
        let edges = self.storage.fetch_edges(ctx, cs_id).await?;
        Ok(edges.map(|edges| {
            edges
                .parents
                .into_iter()
                .map(|parent| parent.cs_id)
                .collect()
        }))
    }

    /// Returns the generation number of a single changeset.
    pub async fn changeset_generation(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Option<Generation>> {
        let edges = self.storage.fetch_edges(ctx, cs_id).await?;
        Ok(edges.map(|edges| edges.node.generation))
    }

    /// Obtain a frontier of changesets from a single changeset id.
    ///
    /// If the changeset does not exist, the frontier is empty.
    async fn single_frontier(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<ChangesetFrontier> {
        let mut frontier = ChangesetFrontier::new();
        if let Some(generation) = self.changeset_generation(ctx, cs_id).await? {
            frontier.insert(generation, hashset! { cs_id });
        }
        Ok(frontier)
    }

    /// Obtain a frontier of changesets from a list of changeset ids.
    #[allow(unused)]
    async fn frontier(
        &self,
        _ctx: &CoreContext,
        _cs_ids: Vec<ChangesetId>,
    ) -> Result<ChangesetFrontier> {
        todo!()
    }

    /// Lower a frontier so that it contains the highest ancestors of the
    /// frontier that have a generation number less than or equal to
    /// `generation`.
    async fn lower_frontier(
        &self,
        ctx: &CoreContext,
        mut frontier: ChangesetFrontier,
        target_generation: Generation,
    ) -> Result<ChangesetFrontier> {
        loop {
            match frontier.last_key_value() {
                None => return Ok(frontier),
                Some((generation, _)) if *generation <= target_generation => {
                    return Ok(frontier);
                }
                _ => {}
            }
            if let Some((_, cs_ids)) = frontier.pop_last() {
                let cs_ids = cs_ids.into_iter().collect::<Vec<_>>();
                let frontier_edges = self
                    .storage
                    .fetch_many_edges(ctx, &cs_ids, Some(target_generation))
                    .await?;
                for cs_id in cs_ids {
                    let edges = frontier_edges
                        .get(&cs_id)
                        .ok_or_else(|| anyhow!("Missing changeset in commit graph: {}", cs_id))?;
                    match edges
                        .merge_ancestor_or_skip_tree_parent
                        // TODO(mbthomas): switch to .skip_tree_parent() once populated
                        .changeset_node()
                        .into_iter()
                        .chain(edges.skip_tree_skew_ancestor)
                        .filter(|ancestor| ancestor.generation >= target_generation)
                        .min_by_key(|ancestor| ancestor.generation)
                    {
                        Some(ancestor) => {
                            frontier
                                .entry(ancestor.generation)
                                .or_default()
                                .insert(ancestor.cs_id);
                        }
                        None => {
                            for parent in edges.parents.iter() {
                                frontier
                                    .entry(parent.generation)
                                    .or_default()
                                    .insert(parent.cs_id);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Returns true if the ancestor changeset is an ancestor of the descendant
    /// changeset.
    ///
    /// Ancestry is inclusive: a commit is its own ancestor.
    pub async fn is_ancestor(
        &self,
        ctx: &CoreContext,
        ancestor: ChangesetId,
        descendant: ChangesetId,
    ) -> Result<bool> {
        let (frontier, target_gen) = futures::try_join!(
            self.single_frontier(ctx, descendant),
            self.changeset_generation(ctx, ancestor)
        )?;
        if let Some(target_gen) = target_gen {
            let frontier = self.lower_frontier(ctx, frontier, target_gen).await?;
            if let Some((frontier_gen, cs_ids)) = frontier.last_key_value() {
                if *frontier_gen == target_gen && cs_ids.contains(&ancestor) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}
