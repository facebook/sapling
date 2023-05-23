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
use anyhow::Result;
use commit_graph_types::edges::ChangesetFrontier;
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

mod compat;
mod core;
mod frontier;

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
