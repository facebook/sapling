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

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use async_trait::async_trait;
use changeset_fetcher::ArcChangesetFetcher;
use changeset_fetcher::ChangesetFetcher;
use context::CoreContext;
use maplit::hashset;
use mononoke_types::ChangesetId;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::Generation;
use smallvec::SmallVec;
use smallvec::ToSmallVec;

use crate::edges::ChangesetEdges;
use crate::edges::ChangesetFrontier;
use crate::edges::ChangesetNode;
use crate::edges::ChangesetNodeParents;
use crate::storage::CommitGraphStorage;

pub mod edges;
pub mod storage;

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

        self.storage
            .add(
                ctx,
                self.build_edges(ctx, cs_id, parents, &parent_edges).await?,
            )
            .await
    }

    /// Same as add but fetches parent edges using the changeset fetcher
    /// if not found in the storage, and recursively tries to add them.
    pub async fn add_recursive(
        &self,
        ctx: &CoreContext,
        changeset_fetcher: ArcChangesetFetcher,
        cs_id: ChangesetId,
        parents: ChangesetParents,
    ) -> Result<usize> {
        let mut edges_map: HashMap<ChangesetId, ChangesetEdges> = Default::default();
        let mut search_stack: Vec<(ChangesetId, ChangesetParents)> = vec![(cs_id, parents)];
        let mut to_add_stack: Vec<(ChangesetId, ChangesetParents)> = Default::default();

        while let Some((cs_id, parents)) = search_stack.pop() {
            to_add_stack.push((cs_id, parents.clone()));

            edges_map.extend(
                self.storage
                    .fetch_many_edges(ctx, &parents, None)
                    .await?
                    .into_iter(),
            );

            for parent in parents {
                if !edges_map.contains_key(&parent) {
                    search_stack.push((
                        parent,
                        changeset_fetcher
                            .get_parents(ctx.clone(), parent)
                            .await?
                            .to_smallvec(),
                    ));
                }
            }
        }

        let mut added_edges_num = 0;

        while let Some((cs_id, parents)) = to_add_stack.pop() {
            let edges = self.build_edges(ctx, cs_id, parents, &edges_map).await?;

            edges_map.insert(cs_id, edges.clone());
            if self.storage.add(ctx, edges).await? {
                added_edges_num += 1;
            }
        }

        Ok(added_edges_num)
    }

    pub async fn build_edges(
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

    /// Returns the parents of a single changeset that must exist.
    pub async fn changeset_parents_required(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<ChangesetParents> {
        self.changeset_parents(ctx, cs_id)
            .await?
            .ok_or_else(|| anyhow!("Missing changeset in commit graph: {}", cs_id))
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

    /// Returns the generation number of a single changeset that must exist.
    pub async fn changeset_generation_required(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Generation> {
        self.changeset_generation(ctx, cs_id)
            .await?
            .ok_or_else(|| anyhow!("Missing changeset in commit graph: {}", cs_id))
    }

    /// Calculates the skew binary ancestor of a changeset
    /// given its parent and two closures, one returns the
    /// skew ancestor of a ChangesetEdges and the other
    /// returns the depth of a ChangesetNode.
    pub async fn calc_skew_ancestor<F, G>(
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

        let parent_edges = self.storage.fetch_edges_required(ctx, parent.cs_id).await?;

        let parent_skew_ancestor = match get_skew_ancestor(&parent_edges) {
            Some(node) => node,
            None => return Ok(Some(parent)),
        };

        let parent_skew_ancestor_edges = self
            .storage
            .fetch_edges_required(ctx, parent_skew_ancestor.cs_id)
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

    /// Returns the ancestor of a changeset that has
    /// depth target_depth, or None if the changeset's
    /// depth is smaller than target_depth.
    pub async fn level_ancestor<F, G, H>(
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
            let node_edges = self.storage.fetch_edges_required(ctx, cs_id).await?;

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

    /// Returns the lowest common ancestor of two changesets.
    pub async fn lowest_common_ancestor<F, G, H>(
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
            self.storage.fetch_edges_required(ctx, cs_id1),
            self.storage.fetch_edges_required(ctx, cs_id2),
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
                self.storage.fetch_edges_required(ctx, u.cs_id),
                self.storage.fetch_edges_required(ctx, v.cs_id),
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

    /// Obtain a frontier of changesets from a single changeset id, which must
    /// exist.
    async fn single_frontier(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<ChangesetFrontier> {
        let generation = self.changeset_generation_required(ctx, cs_id).await?;
        let mut frontier = ChangesetFrontier::new();
        frontier.insert(generation, hashset! { cs_id });
        Ok(frontier)
    }

    /// Obtain a frontier of changesets from a list of changeset ids, which
    /// must all exist.
    async fn frontier(
        &self,
        ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<ChangesetFrontier> {
        let all_edges = self.storage.fetch_many_edges(ctx, &cs_ids, None).await?;

        let mut frontier = ChangesetFrontier::new();
        for cs_id in cs_ids {
            let edges = all_edges
                .get(&cs_id)
                .ok_or_else(|| anyhow!("Missing changeset in commit graph: {}", cs_id))?;
            frontier
                .entry(edges.node.generation)
                .or_default()
                .insert(cs_id);
        }

        Ok(frontier)
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
                        .skip_tree_parent
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
            self.changeset_generation_required(ctx, ancestor)
        )?;
        debug_assert!(!frontier.is_empty(), "frontier should contain descendant");
        let frontier = self.lower_frontier(ctx, frontier, target_gen).await?;
        if let Some((frontier_gen, cs_ids)) = frontier.last_key_value() {
            if *frontier_gen == target_gen && cs_ids.contains(&ancestor) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    // Returns all changesets that are ancestors of any changeset in heads
    // excluding any changeset that is an ancestor of any changeset in common
    pub async fn get_ancestors_difference(
        &self,
        ctx: &CoreContext,
        heads: Vec<ChangesetId>,
        common: Vec<ChangesetId>,
    ) -> Result<Vec<ChangesetId>> {
        let mut cs_ids_inbetween = vec![];

        let (mut heads, mut common) =
            futures::try_join!(self.frontier(ctx, heads), self.frontier(ctx, common))?;

        while let Some((generation, cs_ids)) = heads.pop_last() {
            common = self.lower_frontier(ctx, common, generation).await?;

            let mut cs_ids_not_excluded = vec![];
            for cs_id in cs_ids {
                if let Some((common_frontier_generation, common_cs_ids)) = common.last_key_value() {
                    if *common_frontier_generation == generation && common_cs_ids.contains(&cs_id) {
                        continue;
                    }
                }
                cs_ids_not_excluded.push(cs_id)
            }

            cs_ids_inbetween.extend(&cs_ids_not_excluded);

            let all_edges = self
                .storage
                .fetch_many_edges(ctx, &cs_ids_not_excluded, None)
                .await?;

            for (_, edges) in all_edges.into_iter() {
                for parent in edges.parents.into_iter() {
                    heads
                        .entry(parent.generation)
                        .or_default()
                        .insert(parent.cs_id);
                }
            }
        }

        Ok(cs_ids_inbetween)
    }
}

#[async_trait]
impl ChangesetFetcher for CommitGraph {
    async fn get_generation_number(
        &self,
        ctx: CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Generation> {
        self.changeset_generation_required(&ctx, cs_id).await
    }

    async fn get_parents(&self, ctx: CoreContext, cs_id: ChangesetId) -> Result<Vec<ChangesetId>> {
        self.changeset_parents_required(&ctx, cs_id)
            .await
            .map(SmallVec::into_vec)
    }
}
