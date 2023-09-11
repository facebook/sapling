/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use commit_graph_types::segments::ChangesetSegment;
use commit_graph_types::segments::ChangesetSegmentFrontier;
use commit_graph_types::segments::ChangesetSegmentLocation;
use commit_graph_types::segments::ChangesetSegmentParent;
use commit_graph_types::storage::CommitGraphStorage;
use commit_graph_types::storage::Prefetch;
use context::CoreContext;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use futures_stats::TimedTryFutureExt;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;
use slog::debug;
use smallvec::SmallVec;

use crate::CommitGraph;

/// A set that stores changeset ids and keeps track of all changesets
/// reachable from them by following skew binary ancestor edges.
#[derive(Default, Debug)]
struct SkewAncestorsSet {
    changesets: BTreeMap<Generation, HashSet<ChangesetId>>,
    skew_ancestors: HashMap<ChangesetId, HashSet<ChangesetId>>,
    skew_ancestors_counts: HashMap<ChangesetId, usize>,
}

impl SkewAncestorsSet {
    /// Adds a changeset to the set.
    pub async fn add(
        &mut self,
        ctx: &CoreContext,
        storage: &Arc<dyn CommitGraphStorage>,
        cs_id: ChangesetId,
        base_generation: Generation,
    ) -> Result<()> {
        let mut edges = storage.fetch_edges(ctx, cs_id).await?;

        if self
            .changesets
            .entry(edges.node.generation)
            .or_default()
            .insert(cs_id)
        {
            // if this changeset wasn't already present in the set, increment the count
            // of all changesets reachable by following skew binary ancestors edges.
            loop {
                self.skew_ancestors
                    .entry(cs_id)
                    .or_default()
                    .insert(edges.node.cs_id);
                *self
                    .skew_ancestors_counts
                    .entry(edges.node.cs_id)
                    .or_default() += 1;

                match edges.skip_tree_skew_ancestor {
                    Some(skip_tree_skew_ancestor)
                        if skip_tree_skew_ancestor.generation >= base_generation =>
                    {
                        edges = storage
                            .fetch_edges(ctx, skip_tree_skew_ancestor.cs_id)
                            .await?;
                    }
                    _ => break,
                }
            }
        }

        Ok(())
    }

    /// Returns the highest generation of a changeset in the set,
    /// or None if the set is empty.
    pub fn highest_generation(&self) -> Option<Generation> {
        self.changesets
            .last_key_value()
            .map(|(generation, _)| *generation)
    }

    /// Returns whether the given changeset is reachable from any changeset
    /// in the set by following skew binary ancestor edges.
    pub fn contains_ancestor(&self, cs_id: ChangesetId) -> bool {
        self.skew_ancestors_counts
            .get(&cs_id)
            .map_or(false, |count| *count > 0)
    }

    /// Removes and returns the highest generation number from the set and all changesets
    /// having that generation number.
    pub fn pop_last(&mut self) -> Option<(Generation, HashSet<ChangesetId>)> {
        match self.changesets.pop_last() {
            Some((generation, cs_ids)) => {
                for cs_id in cs_ids.iter() {
                    if let Some(skew_ancestors) = self.skew_ancestors.get(cs_id) {
                        for skew_ancestor in skew_ancestors {
                            if let Some(skew_ancestor_count) =
                                self.skew_ancestors_counts.get_mut(skew_ancestor)
                            {
                                *skew_ancestor_count -= 1;
                            }
                        }
                    }
                }
                Some((generation, cs_ids))
            }
            None => None,
        }
    }
}

impl CommitGraph {
    /// Returns a frontier of segments from each of the given changesets to their
    /// corresponding merge ancestor.
    async fn segment_frontier(
        &self,
        ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<ChangesetSegmentFrontier> {
        let mut frontier: ChangesetSegmentFrontier = Default::default();

        let all_edges = self
            .storage
            .fetch_many_edges(ctx, &cs_ids, Prefetch::None)
            .await?;

        for (cs_id, edges) in all_edges {
            let base = edges.merge_ancestor.unwrap_or(edges.node);
            frontier
                .segments
                .entry(base.generation)
                .or_default()
                .entry(base.cs_id)
                .or_default()
                .insert(cs_id);
        }

        Ok(frontier)
    }

    /// Lower a segment frontier to the specified target generation.
    async fn lower_segment_frontier(
        &self,
        ctx: &CoreContext,
        segment_frontier: &mut ChangesetSegmentFrontier,
        target_generation: Generation,
    ) -> Result<()> {
        loop {
            match segment_frontier.segments.last_key_value() {
                None => return Ok(()),
                Some((generation, _)) if *generation <= target_generation => return Ok(()),
                _ => {}
            }

            if let Some((_generation, segments)) = segment_frontier.segments.pop_last() {
                let segment_bases: Vec<_> = segments.into_keys().collect();
                let all_edges = self
                    .storage
                    .fetch_many_edges(ctx, &segment_bases, Prefetch::None)
                    .await?;

                let parents: Vec<_> = all_edges
                    .into_iter()
                    .flat_map(|(_cs_id, edges)| edges.parents)
                    .map(|node| node.cs_id)
                    .collect();

                let parent_edges = self
                    .storage
                    .fetch_many_edges(ctx, &parents, Prefetch::None)
                    .await?;

                for (cs_id, edges) in parent_edges {
                    let base = edges.merge_ancestor.unwrap_or(edges.node);
                    segment_frontier
                        .segments
                        .entry(base.generation)
                        .or_default()
                        .entry(base.cs_id)
                        .or_default()
                        .insert(cs_id);
                }
            }
        }
    }

    /// Given a list of changesets heads and another list of changesets common, all having
    /// their merge_ancestor pointing to base, returns a list of segments representing all
    /// ancestors of heads, excluding all ancestors of common.
    async fn disjoint_segments(
        &self,
        ctx: &CoreContext,
        base: ChangesetId,
        heads: Vec<ChangesetId>,
        common: Vec<ChangesetId>,
    ) -> Result<Vec<ChangesetSegment>> {
        let base_edges = self.storage.fetch_edges(ctx, base).await?;

        let mut heads_skew_ancestors_set: SkewAncestorsSet = Default::default();
        let mut common_skew_ancestors_set: SkewAncestorsSet = Default::default();

        for cs_id in heads {
            heads_skew_ancestors_set
                .add(ctx, &self.storage, cs_id, base_edges.node.generation)
                .await?;
        }
        for cs_id in common {
            common_skew_ancestors_set
                .add(ctx, &self.storage, cs_id, base_edges.node.generation)
                .await?;
        }

        #[derive(Copy, Clone, Debug)]
        enum Origin {
            Head {
                cs_id: ChangesetId,
                generation: Generation,
            },
            Common,
        }

        let mut frontier: BTreeMap<Generation, HashMap<ChangesetId, Origin>> = Default::default();
        let mut segments = vec![];

        loop {
            // Add the highest generation head changesets to the frontier, if there's
            // no changeset in the frontier or in common with a higher generation number.
            if let Some(heads_generation) = heads_skew_ancestors_set.highest_generation() {
                if frontier
                    .last_key_value()
                    .map_or(true, |(frontier_generation, _)| {
                        heads_generation >= *frontier_generation
                    })
                    && common_skew_ancestors_set
                        .highest_generation()
                        .map_or(true, |common_generation| {
                            heads_generation >= common_generation
                        })
                {
                    if let Some((generation, heads)) = heads_skew_ancestors_set.pop_last() {
                        for cs_id in heads {
                            frontier
                                .entry(generation)
                                .or_default()
                                .entry(cs_id)
                                .or_insert(Origin::Head { cs_id, generation });
                        }
                    }
                }
            }

            // Add the highest generation common changesets to the frontier, if there's
            // no changeset in the frontier with a higher generation number.
            if let Some(common_generation) = common_skew_ancestors_set.highest_generation() {
                if frontier
                    .last_key_value()
                    .map_or(true, |(frontier_generation, _)| {
                        common_generation >= *frontier_generation
                    })
                {
                    if let Some((generation, common)) = common_skew_ancestors_set.pop_last() {
                        for cs_id in common {
                            frontier
                                .entry(generation)
                                .or_default()
                                .insert(cs_id, Origin::Common);
                        }
                    }
                }
            }

            match frontier.pop_last() {
                Some((_generation, last_changesets)) => {
                    let cs_ids: Vec<_> = last_changesets.keys().copied().collect();
                    let all_edges = self
                        .storage
                        .fetch_many_edges(ctx, &cs_ids, Prefetch::None)
                        .await?;

                    // Try to lower the highest generation changesets in the frontier to their
                    // skew binary ancestors, and store any that can't be lowered in either
                    // blocked_heads or blocked_common.

                    let mut immediate_skew_ancestors_count: HashMap<ChangesetId, usize> =
                        Default::default();

                    for cs_id in last_changesets.keys() {
                        let edges = all_edges.get(cs_id).ok_or_else(|| {
                            anyhow!("Missing changeset edges in commit graph for {}", cs_id)
                        })?;

                        if let Some(skew_ancestor) = edges.skip_tree_skew_ancestor {
                            if skew_ancestor.generation >= base_edges.node.generation
                                && !heads_skew_ancestors_set.contains_ancestor(skew_ancestor.cs_id)
                                && !common_skew_ancestors_set.contains_ancestor(skew_ancestor.cs_id)
                            {
                                *immediate_skew_ancestors_count
                                    .entry(skew_ancestor.cs_id)
                                    .or_default() += 1;
                            }
                        }
                    }

                    let mut blocked_heads = vec![];
                    let mut blocked_common = vec![];

                    for (cs_id, origin) in last_changesets.iter() {
                        let edges = all_edges.get(cs_id).ok_or_else(|| {
                            anyhow!("Missing changeset edges in commit graph for {}", cs_id)
                        })?;

                        if let Some(skew_ancestor) = edges.skip_tree_skew_ancestor {
                            if skew_ancestor.generation >= base_edges.node.generation
                                && !heads_skew_ancestors_set.contains_ancestor(skew_ancestor.cs_id)
                                && !common_skew_ancestors_set.contains_ancestor(skew_ancestor.cs_id)
                                && immediate_skew_ancestors_count.get(&skew_ancestor.cs_id)
                                    == Some(&1)
                            {
                                frontier
                                    .entry(skew_ancestor.generation)
                                    .or_default()
                                    .insert(skew_ancestor.cs_id, *origin);
                                continue;
                            }
                        }

                        match origin {
                            Origin::Head {
                                cs_id: origin_cs_id,
                                generation: origin_generation,
                            } => blocked_heads.push((
                                *cs_id,
                                *origin_cs_id,
                                *origin_generation,
                                edges,
                            )),
                            Origin::Common => blocked_common.push((*cs_id, edges)),
                        }
                    }

                    // Lower all blocked common changesets to their immediate parent,
                    // if they are not already at the generation of the base.

                    for (_cs_id, edges) in blocked_common {
                        for parent in edges.parents.iter() {
                            if parent.generation >= base_edges.node.generation {
                                frontier
                                    .entry(parent.generation)
                                    .or_default()
                                    .insert(parent.cs_id, Origin::Common);
                            }
                        }
                    }

                    // Try to lower all blocked head changesets to their immediate parent,
                    // producing a segment for any that can't be lowered due to being blocked
                    // by another changeset in the frontier or a common changeset.

                    for (cs_id, origin_cs_id, origin_generation, edges) in blocked_heads {
                        if edges.node.generation == base_edges.node.generation {
                            segments.push(ChangesetSegment {
                                head: origin_cs_id,
                                base: cs_id,
                                length: origin_generation.value() - edges.node.generation.value()
                                    + 1,
                                parents: edges
                                    .parents
                                    .iter()
                                    .map(|parent| ChangesetSegmentParent {
                                        cs_id: parent.cs_id,
                                        location: Some(ChangesetSegmentLocation {
                                            head: parent.cs_id,
                                            distance: 0,
                                        }),
                                    })
                                    .collect(),
                            });
                            continue;
                        }

                        for parent in edges.parents.iter() {
                            match (
                                frontier
                                    .get(&parent.generation)
                                    .and_then(|segments| segments.get_key_value(&parent.cs_id)),
                                common_skew_ancestors_set.contains_ancestor(parent.cs_id),
                            ) {
                                // Parent is contained in another segment that originates from one of the heads.
                                // Stop extending segment.
                                (
                                    Some((
                                        _,
                                        Origin::Head {
                                            cs_id: parent_segment_origin,
                                            generation: parent_segment_origin_generation,
                                        },
                                    )),
                                    _,
                                ) => segments.push(ChangesetSegment {
                                    head: origin_cs_id,
                                    base: cs_id,
                                    length: origin_generation.value()
                                        - edges.node.generation.value()
                                        + 1,
                                    parents: SmallVec::from(vec![ChangesetSegmentParent {
                                        cs_id: parent.cs_id,
                                        location: Some(ChangesetSegmentLocation {
                                            head: *parent_segment_origin,
                                            distance: parent_segment_origin_generation.value()
                                                - parent.generation.value(),
                                        }),
                                    }]),
                                }),
                                // Parent is an ancestor of common.
                                // Stop extending segment.
                                (Some(_), _) | (_, true) => segments.push(ChangesetSegment {
                                    head: origin_cs_id,
                                    base: cs_id,
                                    length: origin_generation.value()
                                        - edges.node.generation.value()
                                        + 1,
                                    parents: SmallVec::from(vec![ChangesetSegmentParent {
                                        cs_id: parent.cs_id,
                                        location: None,
                                    }]),
                                }),
                                // Parent isn't contained in any other segment, and isn't an ancestor of common.
                                // Continue extending segment.
                                (None, false) => {
                                    frontier.entry(parent.generation).or_default().insert(
                                        parent.cs_id,
                                        Origin::Head {
                                            cs_id: origin_cs_id,
                                            generation: origin_generation,
                                        },
                                    );
                                }
                            }
                        }
                    }
                }
                None => break,
            }
        }

        Ok(segments)
    }

    /// Returns a list of segments representing all ancestors of heads, excluding
    /// all ancestors of common.
    pub async fn ancestors_difference_segments(
        &self,
        ctx: &CoreContext,
        heads: Vec<ChangesetId>,
        common: Vec<ChangesetId>,
    ) -> Result<Vec<ChangesetSegment>> {
        let (mut heads_segment_frontier, mut common_segment_frontier) = futures::try_join!(
            self.segment_frontier(ctx, heads),
            self.segment_frontier(ctx, common)
        )?;

        let mut difference_segments_futures = vec![];

        while let Some((generation, segments)) = heads_segment_frontier.segments.pop_last() {
            self.lower_segment_frontier(ctx, &mut common_segment_frontier, generation)
                .await?;

            let mut bases_not_reachable_from_common = vec![];

            // Go through all the segment bases and calculate the disjoint segments rooted
            // at each base, and for all bases not reachable from common, continue traversing
            // the merge graph.

            for (base, heads) in segments {
                let common = match common_segment_frontier
                    .segments
                    .get(&generation)
                    .and_then(|segments| segments.get(&base))
                {
                    Some(common_segments) => common_segments.iter().copied().collect(),
                    None => {
                        bases_not_reachable_from_common.push(base);
                        vec![]
                    }
                };
                difference_segments_futures.push(self.disjoint_segments(
                    ctx,
                    base,
                    heads.into_iter().collect(),
                    common,
                ));
            }

            let all_edges = self
                .storage
                .fetch_many_edges(ctx, &bases_not_reachable_from_common, Prefetch::None)
                .await?;

            let parents: Vec<_> = all_edges
                .into_iter()
                .flat_map(|(_cs_id, edges)| edges.parents)
                .map(|node| node.cs_id)
                .collect();

            let parent_edges = self
                .storage
                .fetch_many_edges(ctx, &parents, Prefetch::None)
                .await?;

            for (cs_id, edges) in parent_edges {
                let base = edges.merge_ancestor.unwrap_or(edges.node);
                heads_segment_frontier
                    .segments
                    .entry(base.generation)
                    .or_default()
                    .entry(base.cs_id)
                    .or_default()
                    .insert(cs_id);
            }
        }

        stream::iter(difference_segments_futures)
            .buffered(100)
            .map_ok(|segments| stream::iter(segments).map(Ok))
            .try_flatten()
            .try_collect()
            .await
    }

    /// Returns all changesets in a segment in reverse topological order, verifying
    /// that there are no merge changesets in the segment except potentially base,
    /// and that base is an ancestor of head.
    async fn segment_changesets(
        &self,
        ctx: &CoreContext,
        head: ChangesetId,
        base: ChangesetId,
    ) -> Result<Vec<ChangesetId>> {
        let mut segment_cs_ids = vec![];
        let mut current_cs_id = head;

        loop {
            segment_cs_ids.push(current_cs_id);

            if current_cs_id == base {
                break;
            }

            let mut parents = self
                .changeset_parents(ctx, current_cs_id)
                .await?
                .into_iter();

            match (parents.next(), parents.next()) {
                (_, Some(_)) => {
                    return Err(anyhow!(
                        "Found merge changeset {} before segment base",
                        current_cs_id
                    ));
                }
                (None, _) => {
                    return Err(anyhow!(
                        "Segment base {} is not reachable from head {}",
                        base,
                        head
                    ));
                }
                (Some(parent), None) => current_cs_id = parent,
            }
        }

        Ok(segment_cs_ids)
    }

    /// Same as ancestors_difference_segments, but also verifies that:
    /// - The union of all segments matches exactly the returned changesets from ancestors_difference
    /// - All segments are disjoints, no two segments contain the same changeset.
    /// - The parents of each segment are correct.
    /// - No segment contains a merge changeset except potentially at its base.
    /// - Segments are returned in reverse topological order, each parent of each segment either
    /// belong to a subsequent segment or is an ancestor of common.
    pub async fn verified_ancestors_difference_segments(
        &self,
        ctx: &CoreContext,
        heads: Vec<ChangesetId>,
        common: Vec<ChangesetId>,
    ) -> Result<Vec<ChangesetSegment>> {
        let (
            (ancestors_difference_stats, difference_cs_ids),
            (ancestors_difference_segments_stats, difference_segments),
        ) = futures::try_join!(
            self.ancestors_difference(ctx, heads.clone(), common.clone())
                .try_timed(),
            self.ancestors_difference_segments(ctx, heads.clone(), common.clone())
                .try_timed(),
        )?;

        debug!(
            ctx.logger(),
            "ancestors_difference stats {:?}, ancestors_difference_segments stats {:?}",
            ancestors_difference_stats,
            ancestors_difference_segments_stats
        );

        let difference_cs_ids: HashSet<_> = difference_cs_ids.into_iter().collect();

        let mut union_segments_cs_ids: HashMap<_, _> = Default::default();

        for (segment_num, segment) in difference_segments.iter().rev().enumerate() {
            let parents = self.changeset_parents(ctx, segment.base).await?;
            let segment_parents: SmallVec<[ChangesetId; 1]> =
                segment.parents.iter().map(|parent| parent.cs_id).collect();

            if segment_parents != parents {
                return Err(anyhow!(
                    "Incorrect segment parents, expected {:?} found {:?} for segment base {}",
                    segment_parents,
                    parents,
                    segment.base
                ));
            }

            for parent in segment.parents.iter() {
                if difference_cs_ids.contains(&parent.cs_id)
                    && !union_segments_cs_ids.contains_key(&parent.cs_id)
                {
                    return Err(anyhow!(
                        "Segments are not in reverse topological order, segment parent {} not found in any subsequent segment and isn't an ancestor of common",
                        parent,
                    ));
                }

                match (
                    parent.location,
                    union_segments_cs_ids.contains_key(&parent.cs_id),
                ) {
                    // If a location is provided, verify that it resolves to the changeset id.
                    (Some(location), _) => {
                        let location_head_depth = self
                            .storage
                            .fetch_edges(ctx, location.head)
                            .await?
                            .node
                            .skip_tree_depth;
                        let location_level = match location_head_depth.cmp(&location.distance) {
                            Ordering::Less => {
                                return Err(anyhow!(
                                    "Invalid location {}, location head depth is less than location distance",
                                    location,
                                ));
                            }
                            Ordering::Greater | Ordering::Equal => {
                                location_head_depth - location.distance
                            }
                        };
                        let resolved_location = self
                            .skip_tree_level_ancestor(ctx, location.head, location_level)
                            .await?
                            .ok_or_else(|| anyhow!("While resolving location {}", location))?;

                        if resolved_location.cs_id != parent.cs_id {
                            return Err(anyhow!(
                                "Incorrect location for parent of {}, location {} resolves to {}, expected {}",
                                segment.base,
                                location,
                                resolved_location.cs_id,
                                parent.cs_id,
                            ));
                        }
                    }
                    // If the parent belongs to another segment, a location must be provided.
                    (None, true) => {
                        return Err(anyhow!(
                            "Segment parent {} location is None, but it's contained in a subsequent segment",
                            parent.cs_id,
                        ));
                    }
                    _ => {}
                }
            }

            let segment_cs_ids = self
                .segment_changesets(ctx, segment.head, segment.base)
                .await?;

            for cs_id in segment_cs_ids {
                if !difference_cs_ids.contains(&cs_id) {
                    return Err(anyhow!(
                        "Changeset {} in segment {:?} doesn't belong to ancestors difference",
                        cs_id,
                        segment,
                    ));
                }
                if let Some(other_segment) = union_segments_cs_ids.insert(cs_id, segment) {
                    return Err(anyhow!(
                        "Changeset {} found in two segments: {:?}, {:?}",
                        cs_id,
                        segment,
                        other_segment,
                    ));
                }
            }

            if (segment_num + 1) % 1000 == 0 {
                debug!(
                    ctx.logger(),
                    "finished verifying {} segments",
                    segment_num + 1
                );
            }
        }

        debug!(ctx.logger(), "finished verifying all segments");

        if let Some(cs_id) = difference_cs_ids
            .difference(&union_segments_cs_ids.into_keys().collect::<HashSet<_>>())
            .next()
        {
            return Err(anyhow!(
                "Changeset {} found in ancestors difference but is not contained in any segment",
                cs_id,
            ));
        }

        Ok(difference_segments)
    }
}
