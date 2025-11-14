/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Result;
use anyhow::anyhow;
use commit_graph_types::edges::ChangesetEdges;
use commit_graph_types::edges::ChangesetEdgesMut;
use commit_graph_types::edges::ChangesetNode;
use commit_graph_types::edges::ChangesetNodeParents;
use commit_graph_types::edges::ChangesetNodeSubtreeSources;
use commit_graph_types::edges::ChangesetParents;
use commit_graph_types::edges::ChangesetSubtreeSources;
use commit_graph_types::edges::EdgeType;
use commit_graph_types::edges::FirstParentLinear;
use commit_graph_types::edges::Parents;
use commit_graph_types::edges::ParentsAndSubtreeSources;
use commit_graph_types::storage::CommitGraphStorage;
use context::CoreContext;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;

use crate::CommitGraph;
use crate::CommitGraphOps;

impl CommitGraph {
    pub(crate) async fn build_edges(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
        parents: ChangesetParents,
        subtree_sources: ChangesetSubtreeSources,
        edges_map: &HashMap<ChangesetId, ChangesetEdges>,
    ) -> Result<ChangesetEdges> {
        let mut max_parent_gen = 0;
        let mut max_subtree_source_gen = 0;
        let mut edge_parents = ChangesetNodeParents::new();
        let mut merge_ancestor = None;
        let mut subtree_or_merge_ancestor = None;

        let mut skip_tree_parent = None;
        let mut first_parent = true;
        let mut first_subtree_source_or_parent = true;

        let mut p1_linear_depth = 0;

        for parent in &parents {
            let parent_edge = edges_map
                .get(parent)
                .ok_or_else(|| anyhow!("Missing parent: {}", parent))?;
            max_parent_gen = max_parent_gen.max(parent_edge.node().generation::<Parents>().value());
            max_subtree_source_gen = max_subtree_source_gen.max(
                parent_edge
                    .node()
                    .generation::<ParentsAndSubtreeSources>()
                    .value(),
            );
            edge_parents.push(*parent_edge.node());
            if parents.len() == 1 {
                merge_ancestor = Some(
                    *parent_edge
                        .merge_ancestor::<Parents>()
                        .unwrap_or(parent_edge.node()),
                );
                if subtree_sources.is_empty() {
                    subtree_or_merge_ancestor = Some(
                        *parent_edge
                            .merge_ancestor::<ParentsAndSubtreeSources>()
                            .unwrap_or(parent_edge.node()),
                    );
                }
            }

            // skip_tree_parent is the skip tree lowest common ancestor of all parents
            if first_parent {
                first_parent = false;
                skip_tree_parent = Some(*parent_edge.node());

                p1_linear_depth = parent_edge.node().skip_tree_depth::<FirstParentLinear>() + 1;
            } else if let Some(previous_parent) = skip_tree_parent {
                skip_tree_parent = skip_tree_lowest_common_ancestor::<Parents>(
                    ctx,
                    self.storage.as_ref(),
                    previous_parent.cs_id,
                    parent_edge.node().cs_id,
                )
                .await?;
            }
        }

        let mut edge_subtree_sources = ChangesetNodeSubtreeSources::new();

        for source in &subtree_sources {
            let source_edge = edges_map
                .get(source)
                .ok_or_else(|| anyhow!("Missing subtree source: {}", source))?;

            max_subtree_source_gen = max_subtree_source_gen.max(
                source_edge
                    .node()
                    .generation::<ParentsAndSubtreeSources>()
                    .value(),
            );
            edge_subtree_sources.push(*source_edge.node());
        }

        let mut subtree_source_parent = None;

        for node in edge_parents.iter().chain(edge_subtree_sources.iter()) {
            if first_subtree_source_or_parent {
                first_subtree_source_or_parent = false;
                subtree_source_parent = Some(node.clone());
            } else if let Some(previous_source) = subtree_source_parent {
                subtree_source_parent =
                    skip_tree_lowest_common_ancestor::<ParentsAndSubtreeSources>(
                        ctx,
                        self.storage.as_ref(),
                        previous_source.cs_id,
                        node.cs_id,
                    )
                    .await?;
            }
        }

        let generation = Generation::new(max_parent_gen + 1);
        let subtree_source_generation = Generation::new(max_subtree_source_gen + 1);
        let skip_tree_depth = match skip_tree_parent {
            Some(node) => node.skip_tree_depth::<Parents>() + 1,
            None => 0,
        };
        let subtree_source_depth = match subtree_source_parent {
            Some(node) => node.skip_tree_depth::<ParentsAndSubtreeSources>() + 1,
            None => 0,
        };
        let node = ChangesetNode::new(
            cs_id,
            generation,
            subtree_source_generation,
            skip_tree_depth,
            p1_linear_depth,
            subtree_source_depth,
        );

        let p1_parent = edge_parents.first().copied();

        Ok(ChangesetEdgesMut {
            node,
            parents: edge_parents,
            subtree_sources: edge_subtree_sources,
            merge_ancestor,
            skip_tree_parent,
            skip_tree_skew_ancestor: self
                .calc_skew_ancestor::<Parents>(ctx, skip_tree_parent)
                .await?,
            p1_linear_skew_ancestor: self
                .calc_skew_ancestor::<FirstParentLinear>(ctx, p1_parent)
                .await?,
            subtree_or_merge_ancestor,
            subtree_source_parent,
            subtree_source_skew_ancestor: self
                .calc_skew_ancestor::<ParentsAndSubtreeSources>(ctx, subtree_source_parent)
                .await?,
        }
        .freeze())
    }

    /// Calculates the skew binary ancestor of a changeset
    /// given its parent and two closures, one returns the
    /// skew ancestor of a ChangesetEdges and the other
    /// returns the depth of a ChangesetNode.
    async fn calc_skew_ancestor<E: EdgeType>(
        &self,
        ctx: &CoreContext,
        parent: Option<ChangesetNode>,
    ) -> Result<Option<ChangesetNode>> {
        // The skew binary ancestor is either the parent of the
        // changeset or the skew binary ancestor of the skew binary
        // ancestor of the parent if it exists, and if the difference
        // in depth between the parent and the first ancestor is the
        // same as the difference between the two ancestors.

        let parent = match parent {
            Some(node) => node,
            None => return Ok(None),
        };

        let parent_edges = self.storage.fetch_edges(ctx, parent.cs_id).await?;

        let parent_skew_ancestor = match parent_edges.skip_tree_skew_ancestor::<E>() {
            Some(node) => node,
            None => return Ok(Some(parent)),
        };

        let parent_skew_ancestor_edges = self
            .storage
            .fetch_edges(ctx, parent_skew_ancestor.cs_id)
            .await?;

        let parent_second_skew_ancestor =
            match parent_skew_ancestor_edges.skip_tree_skew_ancestor::<E>() {
                Some(node) => node,
                None => return Ok(Some(parent)),
            };

        if parent.skip_tree_depth::<E>() - parent_skew_ancestor.skip_tree_depth::<E>()
            == parent_skew_ancestor.skip_tree_depth::<E>()
                - parent_second_skew_ancestor.skip_tree_depth::<E>()
        {
            Ok(Some(*parent_second_skew_ancestor))
        } else {
            Ok(Some(parent))
        }
    }
}

/// Returns the ancestor of a changeset that has depth target_depth,
/// or None if the changeset's depth is smaller than target_depth.
pub(crate) async fn skip_tree_level_ancestor<E: EdgeType>(
    ctx: &CoreContext,
    storage: &dyn CommitGraphStorage,
    mut cs_id: ChangesetId,
    target_depth: u64,
) -> Result<Option<ChangesetNode>> {
    loop {
        let node_edges = storage.fetch_edges(ctx, cs_id).await?;

        if node_edges.node().skip_tree_depth::<E>() == target_depth {
            return Ok(Some(*node_edges.node()));
        }

        if node_edges.node().skip_tree_depth::<E>() < target_depth {
            return Ok(None);
        }

        match (
            node_edges.skip_tree_skew_ancestor::<E>(),
            node_edges.skip_tree_parent::<E>(),
        ) {
            (Some(skew_ancestor), _) if skew_ancestor.skip_tree_depth::<E>() >= target_depth => {
                cs_id = skew_ancestor.cs_id
            }
            (_, Some(parent)) => cs_id = parent.cs_id,
            _ => {
                return Err(anyhow!(
                    "Changeset has positive depth yet has no parent: {}",
                    cs_id
                ));
            }
        }
    }
}

/// Returns the lowest common ancestor of two changesets in the skip tree.
pub(crate) async fn skip_tree_lowest_common_ancestor<E: EdgeType>(
    ctx: &CoreContext,
    storage: &dyn CommitGraphStorage,
    cs_id1: ChangesetId,
    cs_id2: ChangesetId,
) -> Result<Option<ChangesetNode>> {
    let (edges1, edges2) = futures::try_join!(
        storage.fetch_edges(ctx, cs_id1),
        storage.fetch_edges(ctx, cs_id2),
    )?;

    let (mut u, mut v) = (*edges1.node(), *edges2.node());

    if u.skip_tree_depth::<E>() < v.skip_tree_depth::<E>() {
        std::mem::swap(&mut u, &mut v);
    }

    // Get ancestor of u that has the same depth
    // as v and change u to it
    u = skip_tree_level_ancestor::<E>(ctx, storage, u.cs_id, v.skip_tree_depth::<E>())
        .await?
        .ok_or_else(|| {
            anyhow!(
                "Failed to get ancestor of changeset {} that has depth {}",
                u.cs_id,
                v.skip_tree_depth::<E>(),
            )
        })?;

    // Now that u and v have the same depth, we check if u
    // and v have different skew binary ancestors, if that
    // is the case we move to those ancestors, otherwise we
    // move to their parents. This way we guarantee ending
    // up in the lowest common ancestor.
    while u.cs_id != v.cs_id {
        let (u_edges, v_edges) = futures::try_join!(
            storage.fetch_edges(ctx, u.cs_id),
            storage.fetch_edges(ctx, v.cs_id),
        )?;

        match (
            u_edges.skip_tree_skew_ancestor::<E>(),
            v_edges.skip_tree_skew_ancestor::<E>(),
            u_edges.skip_tree_parent::<E>(),
            v_edges.skip_tree_parent::<E>(),
        ) {
            (Some(u_skew_ancestor), Some(v_skew_ancestor), _, _)
                if u_skew_ancestor.cs_id != v_skew_ancestor.cs_id =>
            {
                u = *u_skew_ancestor;
                v = *v_skew_ancestor;
            }
            (_, _, Some(u_parent), Some(v_parent)) => {
                u = *u_parent;
                v = *v_parent;
            }
            _ => return Ok(None),
        }
    }

    Ok(Some(u))
}

impl<E: EdgeType> CommitGraphOps<E> {
    /// Returns the ancestor of a changeset that has depth target_depth,
    /// or None if the changeset's depth is smaller than target_depth.
    pub async fn skip_tree_level_ancestor(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
        target_depth: u64,
    ) -> Result<Option<ChangesetNode>> {
        skip_tree_level_ancestor::<E>(ctx, self.storage.as_ref(), cs_id, target_depth).await
    }

    /// Returns the lowest common ancestor of two changesets.
    pub async fn skip_tree_lowest_common_ancestor(
        &self,
        ctx: &CoreContext,
        cs_id1: ChangesetId,
        cs_id2: ChangesetId,
    ) -> Result<Option<ChangesetNode>> {
        skip_tree_lowest_common_ancestor::<E>(ctx, self.storage.as_ref(), cs_id1, cs_id2).await
    }
}
