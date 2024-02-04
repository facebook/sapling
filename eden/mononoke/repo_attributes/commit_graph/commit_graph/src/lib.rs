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
use borrowed::borrowed;
use commit_graph_types::edges::ChangesetNode;
use commit_graph_types::edges::ChangesetParents;
use commit_graph_types::storage::CommitGraphStorage;
use commit_graph_types::storage::Prefetch;
use context::CoreContext;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::FuturesUnordered;
use futures::Future;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use itertools::Itertools;
use mononoke_types::ChangesetId;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::Generation;

pub use crate::ancestors_stream::AncestorsStreamBuilder;

mod ancestors_stream;
mod compat;
mod core;
mod frontier;
mod segments;

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
            .fetch_many_edges(ctx, &parents, Prefetch::None)
            .await?
            .into_iter()
            .map(|(k, v)| (k, v.into()))
            .collect();

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
        let edges = self.storage.maybe_fetch_edges(ctx, cs_id).await?;
        Ok(edges.is_some())
    }

    /// Returns the parents of a single changeset.
    pub async fn changeset_parents(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<ChangesetParents> {
        let edges = self.storage.fetch_edges(ctx, cs_id).await?;
        Ok(edges
            .parents
            .into_iter()
            .map(|parent| parent.cs_id)
            .collect())
    }

    /// Returns the generation number of a single changeset.
    pub async fn changeset_generation(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Generation> {
        let edges = self.storage.fetch_edges(ctx, cs_id).await?;
        Ok(edges.node.generation)
    }

    /// Returns a frontier for the ancestors of heads
    /// that satisfy a given property.
    ///
    /// Note: The property needs to be monotonic i.e. if the
    /// property holds for one changeset then it has to hold
    /// for all its parents.
    pub async fn ancestors_frontier_with<'a, MonotonicProperty, Out>(
        &'a self,
        ctx: &'a CoreContext,
        heads: Vec<ChangesetId>,
        monotonic_property: MonotonicProperty,
    ) -> Result<Vec<ChangesetId>>
    where
        MonotonicProperty: Fn(ChangesetId) -> Out + Send + Sync + 'a,
        Out: Future<Output = Result<bool>>,
    {
        let mut ancestors_frontier = vec![];
        let mut frontier = self.frontier(ctx, heads).await?;

        let monotonic_property = move |node: ChangesetNode| {
            borrowed!(monotonic_property);
            monotonic_property(node.cs_id)
        };

        while let Some(ancestors_frontier_extension) = self
            .lower_frontier_step(ctx, &mut frontier, &monotonic_property, Prefetch::None)
            .await?
        {
            ancestors_frontier.extend(ancestors_frontier_extension);
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
            self.changeset_generation(ctx, ancestor)
        )?;
        debug_assert!(!frontier.is_empty(), "frontier should contain descendant");
        self.lower_frontier(ctx, &mut frontier, target_gen).await?;
        Ok(frontier.highest_generation_contains(ancestor, target_gen))
    }

    /// Returns true if the ancestor changeset is an ancestor of any of
    /// the descendant changesets.
    ///
    /// Ancestry is inclusive: a commit is its own ancestor.
    pub async fn is_ancestor_of_any(
        &self,
        ctx: &CoreContext,
        ancestor: ChangesetId,
        descendants: Vec<ChangesetId>,
    ) -> Result<bool> {
        let (mut frontier, target_gen) = futures::try_join!(
            self.frontier(ctx, descendants),
            self.changeset_generation(ctx, ancestor)
        )?;
        self.lower_frontier(ctx, &mut frontier, target_gen).await?;
        Ok(frontier.highest_generation_contains(ancestor, target_gen))
    }

    /// Returns a stream of all ancestors of any changeset in heads,
    /// excluding any ancestor of any changeset in common, in reverse
    /// topological order.
    pub async fn ancestors_difference_stream(
        &self,
        ctx: &CoreContext,
        heads: Vec<ChangesetId>,
        common: Vec<ChangesetId>,
    ) -> Result<BoxStream<'static, Result<ChangesetId>>> {
        AncestorsStreamBuilder::new(Arc::new(self.clone()), ctx.clone(), heads)
            .exclude_ancestors_of(common)
            .build()
            .await
    }

    /// Returns all ancestors of any changeset in heads, excluding any
    /// ancestor of any changeset in common, in reverse topological order.
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

    /// Returns a stream of all changesets that are both descendants of
    /// start_id and ancestors of end_id, in topological order.
    pub async fn range_stream(
        &self,
        ctx: &CoreContext,
        start_id: ChangesetId,
        end_id: ChangesetId,
    ) -> Result<BoxStream<'static, ChangesetId>> {
        let range: Vec<_> =
            AncestorsStreamBuilder::new(Arc::new(self.clone()), ctx.clone(), vec![end_id])
                .descendants_of(start_id)
                .build()
                .await?
                .try_collect()
                .await?;

        Ok(stream::iter(range.into_iter().rev()).boxed())
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
                Some(cs_id) => self.storage.fetch_edges(ctx, *cs_id).await?,
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

    /// Slices ancestors of heads into a sequence of slices for processing.
    ///
    /// Each slice contains a frontier of changesets within a generation range, returning
    /// (slice_start, slice_frontier) corresponds to the frontier that has generations numbers
    /// within [slice_start..(slice_start + slice_size)].
    ///
    /// Useful for any type of processing that needs to happen on ancestors of changesets first.
    /// By processing slices one by one we avoid traversing the entire history all at once.
    ///
    /// The returned slices consist only of frontiers which haven't been processed yet
    /// (determined by the provided needs_processing function). Slicing stops once we
    /// reach a frontier with all its changesets processed.
    pub async fn slice_ancestors<NeedsProcessing, Out>(
        &self,
        ctx: &CoreContext,
        heads: Vec<ChangesetId>,
        needs_processing: NeedsProcessing,
        slice_size: u64,
    ) -> Result<Vec<(Generation, Vec<ChangesetId>)>>
    where
        NeedsProcessing: Fn(Vec<ChangesetId>) -> Out,
        Out: Future<Output = Result<HashSet<ChangesetId>>>,
    {
        let mut frontier = self.frontier(ctx, heads).await?;

        let max_gen = match frontier.last_key_value() {
            Some((gen, _)) => gen,
            None => return Ok(vec![]),
        };

        // The start of the slice is largest number in the sequence
        // 1, slice_size + 1, 2 * slice_size + 1 ...
        let mut slice_start = ((max_gen.value() - 1) / slice_size) * slice_size + 1;

        let mut slices = vec![];

        // Loop over slices in decreasing order of start generation.
        loop {
            let needed_cs_ids = needs_processing(frontier.changesets()).await?;
            frontier = frontier
                .into_flat_iter()
                .filter(|(cs_id, _)| needed_cs_ids.contains(cs_id))
                .collect();

            if frontier.is_empty() {
                break;
            }

            // Only push changesets that are in this slice's range.
            // Any remaining changesets will be pushed in the next iterations.
            slices.push((
                Generation::new(slice_start),
                frontier
                    .changesets_in_range(
                        Generation::new(slice_start)..Generation::new(slice_start + slice_size),
                    )
                    // Sort to make the output deterministic.
                    .sorted()
                    .collect(),
            ));

            if slice_start > 1 {
                // Lower the frontier to the end of the next slice (current slice_start - 1).
                self.lower_frontier(ctx, &mut frontier, Generation::new(slice_start - 1))
                    .await?;
                slice_start -= slice_size;
            } else {
                break;
            }
        }

        Ok(slices.into_iter().rev().collect())
    }

    /// Runs the given `process` closure on all of the given changesets in local topological
    /// order, running as many of them concurrently as possible.
    ///
    /// Note: local topological order here means that all parents of a changeset that are
    /// contained in the input `cs_ids` are processed before itself, but if two changesets
    /// are ancestors of each other and some of the changesets in the path betwen them are
    /// not given in `cs_ids`, they are not guaranteed to be processed in topological order.
    pub async fn process_topologically<Process, Fut>(
        &self,
        ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
        process: Process,
    ) -> Result<()>
    where
        Process: Fn(ChangesetId) -> Fut,
        Fut: Future<Output = Result<()>> + Send,
    {
        // For each changeset in `cs_ids` this stores all other changesets that are in
        // `cs_ids` that are immediate children of it.
        let mut rdeps: HashMap<ChangesetId, HashSet<ChangesetId>> = Default::default();

        // For each changeset in `cs_ids` this stores the number of other changesets in
        // `cs_ids` that are immediate parents of it.
        let mut deps_count: HashMap<ChangesetId, usize> = Default::default();

        let all_edges = self
            .storage
            .fetch_many_edges(ctx, &cs_ids, Prefetch::None)
            .await?;

        let cs_ids = cs_ids.into_iter().collect::<HashSet<_>>();

        for (cs_id, edges) in all_edges.iter() {
            for parent in edges.parents.iter() {
                if cs_ids.contains(&parent.cs_id) {
                    rdeps.entry(parent.cs_id).or_default().insert(*cs_id);
                    *deps_count.entry(*cs_id).or_default() += 1;
                }
            }
        }

        // futs contain a future produced by `process` for all changesets that have
        // no dependencies left. All executing concurrently.
        let mut futs: FuturesUnordered<_> = Default::default();

        for cs_id in cs_ids {
            if !deps_count.contains_key(&cs_id) {
                futs.push(process(cs_id).map_ok(move |()| cs_id).boxed());
            }
        }

        while let Some(result) = futs.next().await {
            let cs_id = result?;

            // After we finish process a changeset, we go through it's reverse dependencies
            // and subtract one from their dependency count. If the count reaches zero, we
            // add a new future to futs to begin processing it.
            let children = rdeps.get(&cs_id).into_iter().flatten();
            for child in children {
                let entry = deps_count.get_mut(child).ok_or_else(|| {
                    anyhow!("deps_count for a child can't be 0 (in process_topologically)")
                })?;
                *entry -= 1;
                if *entry == 0 {
                    futs.push(process(*child).map_ok(move |()| *child).boxed());
                }
            }
        }

        Ok(())
    }

    /// Returns the children of a single changeset.
    pub async fn changeset_children(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Vec<ChangesetId>> {
        self.storage.fetch_children(ctx, cs_id).await
    }
}
