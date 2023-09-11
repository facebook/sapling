/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::anyhow;
use anyhow::Result;
use commit_graph_types::edges::ChangesetEdges;
use commit_graph_types::edges::ChangesetNode;
use commit_graph_types::edges::ChangesetNodeParents;
use commit_graph_types::edges::ChangesetParents;
use context::CoreContext;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;

use crate::CommitGraph;

impl CommitGraph {
    pub(crate) async fn build_edges(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
        parents: ChangesetParents,
        edges_map: &HashMap<ChangesetId, ChangesetEdges>,
    ) -> Result<ChangesetEdges> {
        let mut max_parent_gen = 0;
        let mut edge_parents = ChangesetNodeParents::new();
        let mut merge_ancestor = None;

        let mut skip_tree_parent = None;
        let mut first_parent = true;

        let mut p1_linear_depth = 0;

        for parent in &parents {
            let parent_edge = edges_map
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
                    .lowest_common_ancestor(
                        ctx,
                        previous_parent.cs_id,
                        parent_edge.node.cs_id,
                        |edges| edges.skip_tree_parent,
                        |edges| edges.skip_tree_skew_ancestor,
                        |node| node.skip_tree_depth,
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

        let p1_parent = edge_parents.first().copied();

        Ok(ChangesetEdges {
            node,
            parents: edge_parents,
            merge_ancestor,
            skip_tree_parent,
            skip_tree_skew_ancestor: self
                .calc_skew_ancestor(
                    ctx,
                    skip_tree_parent,
                    |edges| edges.skip_tree_skew_ancestor,
                    |node| node.skip_tree_depth,
                )
                .await?,
            p1_linear_skew_ancestor: self
                .calc_skew_ancestor(
                    ctx,
                    p1_parent,
                    |edges| edges.p1_linear_skew_ancestor,
                    |node| node.p1_linear_depth,
                )
                .await?,
        })
    }

    /// Calculates the skew binary ancestor of a changeset
    /// given its parent and two closures, one returns the
    /// skew ancestor of a ChangesetEdges and the other
    /// returns the depth of a ChangesetNode.
    pub(crate) async fn calc_skew_ancestor<F, G>(
        &self,
        ctx: &CoreContext,
        parent: Option<ChangesetNode>,
        get_skew_ancestor: F,
        get_depth: G,
    ) -> Result<Option<ChangesetNode>>
    where
        F: Fn(&ChangesetEdges) -> Option<ChangesetNode>,
        G: Fn(ChangesetNode) -> u64,
    {
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

        let parent_skew_ancestor = match get_skew_ancestor(&parent_edges) {
            Some(node) => node,
            None => return Ok(Some(parent)),
        };

        let parent_skew_ancestor_edges = self
            .storage
            .fetch_edges(ctx, parent_skew_ancestor.cs_id)
            .await?;

        let parent_second_skew_ancestor = match get_skew_ancestor(&parent_skew_ancestor_edges) {
            Some(node) => node,
            None => return Ok(Some(parent)),
        };

        if get_depth(parent) - get_depth(parent_skew_ancestor)
            == get_depth(parent_skew_ancestor) - get_depth(parent_second_skew_ancestor)
        {
            Ok(Some(parent_second_skew_ancestor))
        } else {
            Ok(Some(parent))
        }
    }

    /// Returns the ancestor of a changeset that has depth target_depth,
    /// or None if the changeset's depth is smaller than target_depth.
    async fn level_ancestor<F, G, H>(
        &self,
        ctx: &CoreContext,
        mut cs_id: ChangesetId,
        target_depth: u64,
        get_parent: F,
        get_skew_ancestor: G,
        get_depth: H,
    ) -> Result<Option<ChangesetNode>>
    where
        F: Fn(&ChangesetEdges) -> Option<ChangesetNode>,
        G: Fn(&ChangesetEdges) -> Option<ChangesetNode>,
        H: Fn(ChangesetNode) -> u64,
    {
        loop {
            let node_edges = self.storage.fetch_edges(ctx, cs_id).await?;

            if get_depth(node_edges.node) == target_depth {
                return Ok(Some(node_edges.node));
            }

            if get_depth(node_edges.node) < target_depth {
                return Ok(None);
            }

            match (get_skew_ancestor(&node_edges), get_parent(&node_edges)) {
                (Some(skew_ancestor), _) if get_depth(skew_ancestor) >= target_depth => {
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

    /// Returns the ancestor of a changeset that has depth target_depth in the skip tree,
    /// or None if the changeset's depth is smaller than target_depth.
    pub async fn skip_tree_level_ancestor(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
        target_depth: u64,
    ) -> Result<Option<ChangesetNode>> {
        self.level_ancestor(
            ctx,
            cs_id,
            target_depth,
            |edges| edges.skip_tree_parent,
            |edges| edges.skip_tree_skew_ancestor,
            |node| node.skip_tree_depth,
        )
        .await
    }

    /// Returns the ancestor of a changeset that has depth target_depth in the p1 linear tree,
    /// or None if the changeset's depth is smaller than target_depth.
    pub async fn p1_linear_level_ancestor(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
        target_depth: u64,
    ) -> Result<Option<ChangesetNode>> {
        self.level_ancestor(
            ctx,
            cs_id,
            target_depth,
            |edges| edges.parents.first().copied(),
            |edges| edges.p1_linear_skew_ancestor,
            |node| node.p1_linear_depth,
        )
        .await
    }

    /// Returns the lowest common ancestor of two changesets.
    async fn lowest_common_ancestor<F, G, H>(
        &self,
        ctx: &CoreContext,
        cs_id1: ChangesetId,
        cs_id2: ChangesetId,
        get_parent: F,
        get_skew_ancestor: G,
        get_depth: H,
    ) -> Result<Option<ChangesetNode>>
    where
        F: Fn(&ChangesetEdges) -> Option<ChangesetNode> + Copy,
        G: Fn(&ChangesetEdges) -> Option<ChangesetNode> + Copy,
        H: Fn(ChangesetNode) -> u64 + Copy,
    {
        let (edges1, edges2) = futures::try_join!(
            self.storage.fetch_edges(ctx, cs_id1),
            self.storage.fetch_edges(ctx, cs_id2),
        )?;

        let (mut u, mut v) = (edges1.node, edges2.node);

        if get_depth(u) < get_depth(v) {
            std::mem::swap(&mut u, &mut v);
        }

        // Get ancestor of u that has the same depth
        // as v and change u to it
        u = self
            .level_ancestor(
                ctx,
                u.cs_id,
                get_depth(v),
                get_parent,
                get_skew_ancestor,
                get_depth,
            )
            .await?
            .ok_or_else(|| {
                anyhow!(
                    "Failed to get ancestor of changeset {} that has depth {}",
                    u.cs_id,
                    get_depth(v)
                )
            })?;

        // Now that u and v have the same depth, we check if u
        // and v have different skew binary ancestors, if that
        // is the case we move to those ancestors, otherwise we
        // move to their parents. This way we guarantee ending
        // up in the lowest common ancestor.
        while u.cs_id != v.cs_id {
            let (u_edges, v_edges) = futures::try_join!(
                self.storage.fetch_edges(ctx, u.cs_id),
                self.storage.fetch_edges(ctx, v.cs_id),
            )?;

            match (
                get_skew_ancestor(&u_edges),
                get_skew_ancestor(&v_edges),
                get_parent(&u_edges),
                get_parent(&v_edges),
            ) {
                (Some(u_skew_ancestor), Some(v_skew_ancestor), _, _)
                    if u_skew_ancestor.cs_id != v_skew_ancestor.cs_id =>
                {
                    u = u_skew_ancestor;
                    v = v_skew_ancestor;
                }
                (_, _, Some(u_parent), Some(v_parent)) => {
                    u = u_parent;
                    v = v_parent;
                }
                _ => return Ok(None),
            }
        }

        Ok(Some(u))
    }

    /// Returns the lowest common ancestor of two changesets in the skip tree.
    pub async fn skip_tree_lowest_common_ancestor(
        &self,
        ctx: &CoreContext,
        cs_id1: ChangesetId,
        cs_id2: ChangesetId,
    ) -> Result<Option<ChangesetNode>> {
        self.lowest_common_ancestor(
            ctx,
            cs_id1,
            cs_id2,
            |edges| edges.skip_tree_parent,
            |edges| edges.skip_tree_skew_ancestor,
            |node| node.skip_tree_depth,
        )
        .await
    }

    /// Returns the lowest common ancestor of two changesets in the p1 linear tree.
    pub async fn p1_linear_lowest_common_ancestor(
        &self,
        ctx: &CoreContext,
        cs_id1: ChangesetId,
        cs_id2: ChangesetId,
    ) -> Result<Option<ChangesetNode>> {
        self.lowest_common_ancestor(
            ctx,
            cs_id1,
            cs_id2,
            |edges| edges.skip_tree_parent,
            |edges| edges.skip_tree_skew_ancestor,
            |node| node.skip_tree_depth,
        )
        .await
    }
}
