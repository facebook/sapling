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
        let parent_edges = self
            .storage
            .fetch_many_edges_required(ctx, &parents, None)
            .await?;

        let mut max_parent_gen = 0;
        let mut edge_parents = ChangesetNodeParents::new();
        let mut merge_ancestor = None;

        let mut skip_tree_parent = None;
        let mut first_parent = true;

        let mut p1_linear_depth = 0;

        for parent in &parents {
            let parent_edge = parent_edges
                .get(parent)
                .ok_or_else(|| anyhow!("Missing parent: {}", parent))?;
            max_parent_gen = max_parent_gen.max(parent_edge.node.generation.value());
            edge_parents.push(parent_edge.node);
            if parents.len() == 1 {
                merge_ancestor = Some(parent_edge.merge_ancestor.unwrap_or(parent_edge.node));
            }

            // skip_tree_parent is the skip tree lowest common ancestor of all parents
            if first_parent {
                first_parent = false;
                skip_tree_parent = Some(parent_edge.node);

                p1_linear_depth = parent_edge.node.p1_linear_depth + 1;
            } else if let Some(previous_parent) = skip_tree_parent {
                skip_tree_parent = self
                    .skip_tree_lowest_common_ancestor(
                        ctx,
                        previous_parent.cs_id,
                        parent_edge.node.cs_id,
                    )
                    .await?;
            }
        }

        let generation = Generation::new(max_parent_gen + 1);
        let skip_tree_depth = match skip_tree_parent {
            Some(node) => node.skip_tree_depth + 1,
            None => 0,
        };
        let node = ChangesetNode {
            cs_id,
            generation,
            skip_tree_depth,
            p1_linear_depth,
        };

        let edges = ChangesetEdges {
            node,
            parents: edge_parents,
            merge_ancestor,
            skip_tree_parent,
            skip_tree_skew_ancestor: self
                .calc_skip_tree_skew_ancestor(ctx, skip_tree_parent)
                .await?,
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

    /// Calculates the skew binary ancestor of a changeset
    /// in the skip tree, given its direct skip tree parent.
    pub async fn calc_skip_tree_skew_ancestor(
        &self,
        ctx: &CoreContext,
        skip_tree_parent: Option<ChangesetNode>,
    ) -> Result<Option<ChangesetNode>> {
        // The skew binary ancestor is either the parent of the
        // changeset or the skew binary ancestor of the skew binary
        // ancestor of the parent if it exists, and if the difference
        // in depth between the parent and the first ancestor is the
        // same as the difference between the two ancestors.

        let skip_tree_parent = match skip_tree_parent {
            Some(node) => node,
            None => return Ok(None),
        };

        let skip_tree_parent_edges = self
            .storage
            .fetch_edges_required(ctx, skip_tree_parent.cs_id)
            .await?;

        let skip_tree_parent_skew_binary_ancestor =
            match skip_tree_parent_edges.skip_tree_skew_ancestor {
                Some(node) => node,
                None => return Ok(Some(skip_tree_parent)),
            };

        let skip_tree_parent_skew_binary_ancestor_edges = self
            .storage
            .fetch_edges_required(ctx, skip_tree_parent_skew_binary_ancestor.cs_id)
            .await?;

        let skip_tree_parent_second_skew_binary_ancestor =
            match skip_tree_parent_skew_binary_ancestor_edges.skip_tree_skew_ancestor {
                Some(node) => node,
                None => return Ok(Some(skip_tree_parent)),
            };

        if skip_tree_parent.skip_tree_depth - skip_tree_parent_skew_binary_ancestor.skip_tree_depth
            == skip_tree_parent_skew_binary_ancestor.skip_tree_depth
                - skip_tree_parent_second_skew_binary_ancestor.skip_tree_depth
        {
            Ok(Some(skip_tree_parent_second_skew_binary_ancestor))
        } else {
            Ok(Some(skip_tree_parent))
        }
    }

    /// Returns the skip tree ancestor of a changeset that has
    /// depth target_depth, or None if the changeset's
    /// skip_tree_depth is smaller than target_depth.
    pub async fn skip_tree_level_ancestor(
        &self,
        ctx: &CoreContext,
        mut cs_id: ChangesetId,
        target_depth: u64,
    ) -> Result<Option<ChangesetNode>> {
        loop {
            let node_edges = self.storage.fetch_edges_required(ctx, cs_id).await?;

            if node_edges.node.skip_tree_depth == target_depth {
                return Ok(Some(node_edges.node));
            }

            if node_edges.node.skip_tree_depth < target_depth {
                return Ok(None);
            }

            match (
                node_edges.skip_tree_skew_ancestor,
                node_edges.skip_tree_parent,
            ) {
                (Some(skew_ancestor), _) if skew_ancestor.skip_tree_depth >= target_depth => {
                    cs_id = skew_ancestor.cs_id
                }
                (_, Some(skip_parent)) => cs_id = skip_parent.cs_id,
                _ => {
                    return Err(anyhow!(
                        "Changeset has positive skip_tree_depth yet has no skip_tree_parent: {}",
                        cs_id
                    ));
                }
            }
        }
    }

    /// Returns the lowest common ancestor of two changesets in the skip tree.
    pub async fn skip_tree_lowest_common_ancestor(
        &self,
        ctx: &CoreContext,
        cs_id1: ChangesetId,
        cs_id2: ChangesetId,
    ) -> Result<Option<ChangesetNode>> {
        let (edges1, edges2) = futures::try_join!(
            self.storage.fetch_edges_required(ctx, cs_id1),
            self.storage.fetch_edges_required(ctx, cs_id2),
        )?;

        let (mut u, mut v) = (edges1.node, edges2.node);

        if u.skip_tree_depth < v.skip_tree_depth {
            std::mem::swap(&mut u, &mut v);
        }

        // Get ancestor of u that has the same skip_tree_depth
        // as v and change u to it
        u = self
            .skip_tree_level_ancestor(ctx, u.cs_id, v.skip_tree_depth)
            .await?
            .ok_or_else(|| {
                anyhow!(
                    "Failed to get ancestor of changeset {} that has depth {}",
                    u.cs_id,
                    u.skip_tree_depth
                )
            })?;

        // Now that u and v have the same skip_tree_depth,
        // we check if u and v have different skew binary
        // ancestors, if that is the case we move to those ancestors,
        // otherwise we move to their skip tree parents. This way
        // we guarantee ending up in the lowest common ancestor.
        while u.cs_id != v.cs_id {
            let (u_edges, v_edges) = futures::try_join!(
                self.storage.fetch_edges_required(ctx, u.cs_id),
                self.storage.fetch_edges_required(ctx, v.cs_id),
            )?;

            match (
                u_edges.skip_tree_skew_ancestor,
                v_edges.skip_tree_skew_ancestor,
                u_edges.skip_tree_parent,
                v_edges.skip_tree_parent,
            ) {
                (Some(u_skew_ancestor), Some(v_skew_ancestor), _, _)
                    if u_skew_ancestor.cs_id != v_skew_ancestor.cs_id =>
                {
                    u = u_skew_ancestor;
                    v = v_skew_ancestor;
                }
                (_, _, Some(u_skip_parent), Some(v_skip_parent)) => {
                    u = u_skip_parent;
                    v = v_skip_parent;
                }
                _ => return Ok(None),
            }
        }

        Ok(Some(u))
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
                    .fetch_many_edges_required(ctx, &cs_ids, Some(target_generation))
                    .await?;
                for cs_id in cs_ids {
                    let edges = frontier_edges
                        .get(&cs_id)
                        .ok_or_else(|| anyhow!("Missing changeset in commit graph: {}", cs_id))?;
                    match edges
                        .merge_ancestor
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
