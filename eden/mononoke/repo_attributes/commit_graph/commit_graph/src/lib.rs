/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Commit Graph
//!
//! The graph of all commits in the repository.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use buffered_commit_graph_storage::BufferedCommitGraphStorage;
use changeset_fetcher::ArcChangesetFetcher;
use changeset_fetcher::ChangesetFetcher;
use commit_graph_types::edges::ChangesetEdges;
use commit_graph_types::edges::ChangesetFrontier;
use commit_graph_types::edges::ChangesetNode;
use commit_graph_types::edges::ChangesetNodeParents;
use commit_graph_types::storage::CommitGraphStorage;
use commit_graph_types::storage::Prefetch;
use commit_graph_types::ChangesetParents;
use context::CoreContext;
use futures::stream;
use futures::stream::BoxStream;
use futures::Stream;
use futures::StreamExt;
use futures::TryStreamExt;
use itertools::Either;
use itertools::Itertools;
use mononoke_types::ChangesetId;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::Generation;
use smallvec::SmallVec;
use smallvec::ToSmallVec;
use vec1::Vec1;

/// Commit Graph.
///
/// This contains the graph of all commits known to Mononoke for a particular
/// repository.  It provides methods for traversing the commit graph and
/// finding out graph-related information for the changesets contained
/// therein.
#[derive(Clone)]
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
            .fetch_many_edges_required(ctx, &parents, Prefetch::None)
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
    ///
    /// Changesets should be sorted in topological order.
    pub async fn add_recursive(
        &self,
        ctx: &CoreContext,
        changeset_fetcher: ArcChangesetFetcher,
        changesets: Vec1<(ChangesetId, ChangesetParents)>,
    ) -> Result<usize> {
        let mut edges_map: HashMap<ChangesetId, ChangesetEdges> = Default::default();
        let changesets_set: HashSet<ChangesetId> =
            changesets.iter().map(|(cs_id, _)| cs_id).cloned().collect();
        let mut search_stack: Vec<(ChangesetId, ChangesetParents)> = changesets.into();
        let mut to_add_stack: Vec<(ChangesetId, ChangesetParents)> = Default::default();

        while let Some((cs_id, parents)) = search_stack.pop() {
            // If edges map already has the key there's no need to process it (this may happen if
            // initial vector had duplicates or if we descent into the same parrents via two
            // different paths)
            if edges_map.contains_key(&cs_id) {
                continue;
            }

            to_add_stack.push((cs_id, parents.clone()));

            // We don't need to look up:
            //  * changesets we already have in edges_map
            //  * changesets that are part of changesets set (as they'll be inserted anyway)
            let parents_to_fetch: SmallVec<[ChangesetId; 1]> = parents
                .into_iter()
                .filter(|cs_id| !edges_map.contains_key(cs_id) && !changesets_set.contains(cs_id))
                .collect();

            if !parents_to_fetch.is_empty() {
                edges_map.extend(
                    self.storage
                        .fetch_many_edges(ctx, &parents_to_fetch, Prefetch::None)
                        .await
                        .with_context(|| "during commit_graph::add_recursive (fetch_many_edges)")?
                        .into_iter(),
                );
            }

            for parent in parents_to_fetch {
                if !edges_map.contains_key(&parent) {
                    // If the parents are not present in the commit graph we have to backfilll them
                    // so let's add them to the stack so they can be processed in the next
                    // iteration.
                    search_stack.push((
                        parent,
                        changeset_fetcher
                            .get_parents(ctx, parent)
                            .await
                            .with_context(|| "during commit_graph::add_recursive (get_parents)")?
                            .to_smallvec(),
                    ));
                }
            }
        }

        // We use buffered storage here to be able to do all the writes in parallel.
        // We need to create a new CommitGraph wrapper to work with the buffered storage.
        let buffered_storage =
            Arc::new(BufferedCommitGraphStorage::new(self.storage.clone(), 10000));
        let graph = CommitGraph::new(buffered_storage.clone());
        while let Some((cs_id, parents)) = to_add_stack.pop() {
            let edges = graph.build_edges(ctx, cs_id, parents, &edges_map).await?;
            edges_map.insert(cs_id, edges.clone());
            buffered_storage.add(ctx, edges).await?;
        }
        buffered_storage.flush(ctx).await
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

    /// Obtain a frontier of changesets from a single changeset id, which must
    /// exist.
    async fn single_frontier(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<ChangesetFrontier> {
        let generation = self.changeset_generation_required(ctx, cs_id).await?;
        Ok(ChangesetFrontier::new_single(cs_id, generation))
    }

    /// Obtain a frontier of changesets from a list of changeset ids, which
    /// must all exist.
    async fn frontier(
        &self,
        ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<ChangesetFrontier> {
        let all_edges = self
            .storage
            .fetch_many_edges(ctx, &cs_ids, Prefetch::None)
            .await?;

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
        frontier: &mut ChangesetFrontier,
        target_generation: Generation,
    ) -> Result<()> {
        loop {
            match frontier.last_key_value() {
                None => return Ok(()),
                Some((generation, _)) if *generation <= target_generation => {
                    return Ok(());
                }
                _ => {}
            }
            if let Some((_, cs_ids)) = frontier.pop_last() {
                let cs_ids = cs_ids.into_iter().collect::<Vec<_>>();
                let frontier_edges = self
                    .storage
                    .fetch_many_edges_required(
                        ctx,
                        &cs_ids,
                        Prefetch::for_skip_tree_traversal(target_generation),
                    )
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

    /// Lower the highest generation changesets of a frontier
    /// to their immediate parents.
    async fn lower_frontier_highest_generation(
        &self,
        ctx: &CoreContext,
        frontier: &mut ChangesetFrontier,
    ) -> Result<()> {
        if let Some((_, cs_ids)) = frontier.pop_last() {
            let cs_ids = cs_ids.into_iter().collect::<Vec<_>>();
            let frontier_edges = self
                .storage
                .fetch_many_edges_required(ctx, &cs_ids, Prefetch::for_p1_linear_traversal())
                .await?;

            for cs_id in cs_ids {
                let edges = frontier_edges
                    .get(&cs_id)
                    .ok_or_else(|| anyhow!("Missing changeset in commit graph: {}", cs_id))?;

                for parent in edges.parents.iter() {
                    frontier
                        .entry(parent.generation)
                        .or_default()
                        .insert(parent.cs_id);
                }
            }
        }
        Ok(())
    }

    /// Returns a frontier for the ancestors of heads
    /// that satisfy a given property.
    ///
    /// Note: The property needs to be monotonic i.e. if the
    /// property holds for one changeset then it has to hold
    /// for all its parents.
    pub async fn ancestors_frontier_with(
        &self,
        ctx: &CoreContext,
        heads: Vec<ChangesetId>,
        monotonic_property: impl Fn(ChangesetId) -> bool,
    ) -> Result<Vec<ChangesetId>> {
        let (mut ancestors_frontier, frontier_cs_ids): (HashSet<_>, Vec<_>) =
            heads.into_iter().partition_map(|cs_id| {
                if monotonic_property(cs_id) {
                    Either::Left(cs_id)
                } else {
                    Either::Right(cs_id)
                }
            });
        let mut frontier = self.frontier(ctx, frontier_cs_ids).await?;

        while let Some((_, cs_ids)) = frontier.pop_last() {
            let cs_ids = cs_ids.into_iter().collect::<Vec<_>>();
            let frontier_edges = self
                .storage
                .fetch_many_edges_required(ctx, &cs_ids, Prefetch::None)
                .await?;
            for cs_id in cs_ids {
                let edges = frontier_edges
                    .get(&cs_id)
                    .ok_or_else(|| anyhow!("Missing changeset in commit graph: {}", cs_id))?;
                match edges
                    .skip_tree_parent
                    .into_iter()
                    .chain(edges.skip_tree_skew_ancestor)
                    .filter(|node| !monotonic_property(node.cs_id))
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
                            if monotonic_property(parent.cs_id) {
                                ancestors_frontier.insert(parent.cs_id);
                            } else {
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

        Ok(ancestors_frontier.into_iter().collect())
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
        let (mut frontier, target_gen) = futures::try_join!(
            self.single_frontier(ctx, descendant),
            self.changeset_generation_required(ctx, ancestor)
        )?;
        debug_assert!(!frontier.is_empty(), "frontier should contain descendant");
        self.lower_frontier(ctx, &mut frontier, target_gen).await?;
        Ok(frontier.highest_generation_contains(ancestor, target_gen))
    }

    pub async fn ancestors_difference_stream_with(
        &self,
        ctx: &CoreContext,
        heads: Vec<ChangesetId>,
        common: Vec<ChangesetId>,
        monotonic_property: impl Fn(ChangesetId) -> bool + 'static,
    ) -> Result<impl Stream<Item = Result<ChangesetId>>> {
        struct AncestorsDifferenceState<P: Fn(ChangesetId) -> bool> {
            heads: ChangesetFrontier,
            common: ChangesetFrontier,
            monotonic_property: P,
        }

        let (heads, common) =
            futures::try_join!(self.frontier(ctx, heads), self.frontier(ctx, common))?;

        // Given that `self` is Arc under the hood, it's cheap to clone it
        let (this, ctx) = (self.clone(), ctx.clone());
        Ok(stream::try_unfold(
            Box::new(AncestorsDifferenceState {
                heads,
                common,
                monotonic_property,
            }),
            move |mut state| {
                let (this, ctx) = (this.clone(), ctx.clone());
                async move {
                    if let Some((generation, cs_ids)) = state.heads.pop_last() {
                        this.lower_frontier(&ctx, &mut state.common, generation)
                            .await?;

                        let mut cs_ids_not_excluded = vec![];
                        for cs_id in cs_ids {
                            if !state.common.highest_generation_contains(cs_id, generation)
                                && !(state.monotonic_property)(cs_id)
                            {
                                cs_ids_not_excluded.push(cs_id)
                            }
                        }

                        let all_edges = this
                            .storage
                            .fetch_many_edges(
                                &ctx,
                                &cs_ids_not_excluded,
                                Prefetch::for_p1_linear_traversal(),
                            )
                            .await?;

                        for (_, edges) in all_edges.into_iter() {
                            for parent in edges.parents.into_iter() {
                                state
                                    .heads
                                    .entry(parent.generation)
                                    .or_default()
                                    .insert(parent.cs_id);
                            }
                        }

                        anyhow::Ok(Some((stream::iter(cs_ids_not_excluded).map(Ok), state)))
                    } else {
                        Ok(None)
                    }
                }
            },
        )
        .try_flatten())
    }

    pub async fn ancestors_difference_stream(
        &self,
        ctx: &CoreContext,
        heads: Vec<ChangesetId>,
        common: Vec<ChangesetId>,
    ) -> Result<impl Stream<Item = Result<ChangesetId>>> {
        self.ancestors_difference_stream_with(ctx, heads, common, |_| false)
            .await
    }

    /// Returns all ancestors of any changeset in heads, excluding
    /// any ancestor of any changeset in common and any changeset
    /// that satisfies a given property.
    ///
    /// Note: The property needs to be monotonic i.e. if the
    /// property holds for one changeset then it has to hold
    /// for all its parents.
    pub async fn ancestors_difference_with(
        &self,
        ctx: &CoreContext,
        heads: Vec<ChangesetId>,
        common: Vec<ChangesetId>,
        monotonic_property: impl Fn(ChangesetId) -> bool + 'static,
    ) -> Result<Vec<ChangesetId>> {
        self.ancestors_difference_stream_with(ctx, heads, common, monotonic_property)
            .await?
            .try_collect()
            .await
    }

    /// Returns all ancestors of any changeset in heads, excluding
    /// any ancestor of any changeset in common.
    pub async fn ancestors_difference(
        &self,
        ctx: &CoreContext,
        heads: Vec<ChangesetId>,
        common: Vec<ChangesetId>,
    ) -> Result<Vec<ChangesetId>> {
        self.ancestors_difference_stream(ctx, heads, common)
            .await?
            .try_collect()
            .await
    }

    pub async fn range_stream<'a>(
        &'a self,
        ctx: &'a CoreContext,
        start_id: ChangesetId,
        end_id: ChangesetId,
    ) -> Result<BoxStream<'a, ChangesetId>> {
        let (start_generation, mut frontier) = futures::try_join!(
            self.changeset_generation_required(ctx, start_id),
            self.single_frontier(ctx, end_id)
        )?;
        let mut children: HashMap<ChangesetId, HashSet<(ChangesetId, Generation)>> =
            Default::default();
        let mut reached_start = false;

        while let Some((gen, cs_ids)) = frontier.pop_last() {
            let cs_ids = cs_ids.into_iter().collect::<Vec<_>>();
            let all_edges = self
                .storage
                .fetch_many_edges_required(ctx, &cs_ids, Prefetch::for_p1_linear_traversal())
                .await?;

            reached_start |= cs_ids.contains(&start_id);

            if gen > start_generation {
                for (_, edges) in all_edges.into_iter() {
                    for parent in edges.parents.into_iter() {
                        children
                            .entry(parent.cs_id)
                            .or_default()
                            .insert((edges.node.cs_id, edges.node.generation));
                        frontier
                            .entry(parent.generation)
                            .or_default()
                            .insert(parent.cs_id);
                    }
                }
            }
        }

        if !reached_start {
            return Ok(stream::empty().boxed());
        }

        struct RangeStreamState {
            children: HashMap<ChangesetId, HashSet<(ChangesetId, Generation)>>,
            upwards_frontier: ChangesetFrontier,
        }

        Ok(stream::unfold(
            Box::new(RangeStreamState {
                children,
                upwards_frontier: ChangesetFrontier::new_single(start_id, start_generation),
            }),
            |mut state| async {
                if let Some((_, cs_ids)) = state.upwards_frontier.pop_first() {
                    for cs_id in cs_ids.iter() {
                        if let Some(children) = state.children.get(cs_id) {
                            for (child, generation) in children.iter() {
                                state
                                    .upwards_frontier
                                    .entry(*generation)
                                    .or_default()
                                    .insert(*child);
                            }
                        }
                    }
                    Some((stream::iter(cs_ids), state))
                } else {
                    None
                }
            },
        )
        .flatten()
        .boxed())
    }

    /// Returns all of the highest generation changesets that
    /// are ancestors of both u and v, sorted by changeset id.
    pub async fn common_base(
        &self,
        ctx: &CoreContext,
        u: ChangesetId,
        v: ChangesetId,
    ) -> Result<Vec<ChangesetId>> {
        let (mut u_frontier, mut v_frontier) =
            futures::try_join!(self.single_frontier(ctx, u), self.single_frontier(ctx, v))?;

        loop {
            let u_gen = match u_frontier.last_key_value() {
                Some((gen, _)) => *gen,
                // if u_frontier is empty then there are no common ancestors.
                None => return Ok(vec![]),
            };

            // lower v_frontier to the highest generation of u_frontier
            self.lower_frontier(ctx, &mut v_frontier, u_gen).await?;

            // Check if the highest generation of u_frontier intersects with v_frontier
            // and return the intersection if so.
            let mut intersection = u_frontier.highest_generation_intersection(&v_frontier);
            if !intersection.is_empty() {
                intersection.sort();
                return Ok(intersection);
            }

            let u_highest_generation_edges = match u_frontier
                .last_key_value()
                .and_then(|(_, cs_ids)| cs_ids.iter().next())
            {
                Some(cs_id) => self.storage.fetch_edges_required(ctx, *cs_id).await?,
                None => return Ok(vec![]),
            };

            // Try to lower u_frontier to the generation of one of its
            // highest generation changesets' skip tree skew ancestor.
            // This is optimized for the case where u_frontier has only
            // one changeset, but is correct in all cases.
            if let Some(ancestor) = u_highest_generation_edges.skip_tree_skew_ancestor {
                let mut lowered_u_frontier = u_frontier.clone();
                let mut lowered_v_frontier = v_frontier.clone();

                self.lower_frontier(ctx, &mut lowered_u_frontier, ancestor.generation)
                    .await?;
                self.lower_frontier(ctx, &mut lowered_v_frontier, ancestor.generation)
                    .await?;

                // If the two lowered frontier are disjoint then it's safe to lower,
                // otherwise there might be a higher generation common ancestor.
                if lowered_u_frontier.is_disjoint(&lowered_v_frontier) {
                    u_frontier = lowered_u_frontier;
                    v_frontier = lowered_v_frontier;

                    continue;
                }
            }

            // If we could lower u_frontier using the skip tree skew ancestor
            // lower only the highest generation instead.
            self.lower_frontier_highest_generation(ctx, &mut u_frontier)
                .await?;
        }
    }
}

#[async_trait]
impl ChangesetFetcher for CommitGraph {
    async fn get_generation_number(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Generation> {
        self.changeset_generation_required(ctx, cs_id).await
    }

    async fn get_parents(&self, ctx: &CoreContext, cs_id: ChangesetId) -> Result<Vec<ChangesetId>> {
        self.changeset_parents_required(ctx, cs_id)
            .await
            .map(SmallVec::into_vec)
    }
}
