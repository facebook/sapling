/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Result;
use commit_graph_types::edges::ChangesetNode;
use commit_graph_types::storage::Prefetch;
use commit_graph_types::storage::PrefetchTarget;
use context::CoreContext;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use mononoke_types::ChangesetId;
use mononoke_types::FIRST_GENERATION;

use crate::ArcCommitGraph;
use crate::CommitGraph;

impl CommitGraph {
    /// Returns a stream of the linear ancestors of a changeset, starting
    /// at distance `start_distance` from the given changeset, and optionally
    /// ending at distance `end_distance`.
    ///
    /// If start_distance is greater than the distance of the changeset from
    /// the root, then it will return an empty stream.
    async fn linear_ancestors_stream(
        &self,
        ctx: CoreContext,
        cs_id: ChangesetId,
        start_distance: u64,
        end_distance: Option<u64>,
    ) -> Result<BoxStream<'static, Result<ChangesetId>>> {
        let edges = self.storage.fetch_edges(&ctx, cs_id).await?;

        if edges.node.p1_linear_depth < start_distance {
            return Ok(stream::empty().boxed());
        }

        if let Some(end_distance) = end_distance {
            if end_distance <= start_distance {
                return Ok(stream::empty().boxed());
            }
        }

        // Find the linear ancestor that's at `start_distance` from the given changeset.
        let first_ancestor = self
            .p1_linear_level_ancestor(&ctx, cs_id, edges.node.p1_linear_depth - start_distance)
            .await?
            .ok_or_else(|| {
                anyhow!(
                    "Failed to find p1 linear level ancestor for {} at depth {}",
                    cs_id,
                    edges.node.p1_linear_depth - start_distance,
                )
            })?
            .cs_id;

        struct LinearAncestorsStreamState {
            commit_graph: CommitGraph,
            ctx: CoreContext,
            ancestor: Option<ChangesetId>,
            count: u64,
        }

        Ok(stream::try_unfold(LinearAncestorsStreamState {
            commit_graph: self.clone(),
            ctx,
            ancestor: Some(first_ancestor),
            count: end_distance.map_or(edges.node.p1_linear_depth - start_distance + 1, |end_distance| end_distance - start_distance),
        }, move |state| async move {
            let LinearAncestorsStreamState {
                commit_graph,
                ctx,
                ancestor,
                count,
            } = state;

            let ancestor = match ancestor {
                Some(ancestor) => ancestor,
                None => return Ok(None),
            };

            if count == 0 {
                return Ok(None);
            }

            let ancestor_edges = commit_graph
                .storage
                .fetch_many_edges(&ctx, &[ancestor], Prefetch::Hint(PrefetchTarget::LinearAncestors {
                    generation: FIRST_GENERATION,
                    steps: count,
                }))
                .await?
                .remove(&ancestor)
                .ok_or_else(|| anyhow!("Missing changeset from commit graph storage: {} (linear_ancestors_stream)", ancestor))?
                .edges();

            Ok(Some((ancestor, LinearAncestorsStreamState {
                commit_graph,
                ctx,
                ancestor: ancestor_edges.parents.into_iter().next().map(|node| node.cs_id),
                count: count - 1,
            })))
        }).boxed())
    }
}

/// A builder for a stream of linear ancestors of a changeset.
///
/// The builder allows constraining the stream to exclude the linear
/// ancestors of a changeset, or to include only the linear descendants
/// of a changeset. It additionally allows efficiently skipping the first
/// N linear ancestors of the stream.
pub struct LinearAncestorsStreamBuilder {
    commit_graph: ArcCommitGraph,
    ctx: CoreContext,
    head: ChangesetNode,
    start_distance: u64,
    end_distance: Option<u64>,
}

impl LinearAncestorsStreamBuilder {
    pub async fn new(
        commit_graph: ArcCommitGraph,
        ctx: CoreContext,
        head: ChangesetId,
    ) -> Result<Self> {
        let head = commit_graph.changeset_node(&ctx, head).await?;
        Ok(Self {
            commit_graph,
            ctx,
            head,
            start_distance: 0,
            end_distance: None,
        })
    }

    /// Exclude all linear ancestors of the given changeset from the stream.
    pub async fn exclude_ancestors_of(mut self, common: ChangesetId) -> Result<Self> {
        // The common linear ancestors between the head of the stream and the given changeset
        // are all the ancestors of their lowest common ancestor.
        let lowest_common_ancestor = self
            .commit_graph
            .p1_linear_lowest_common_ancestor(&self.ctx, self.head.cs_id, common)
            .await?;

        if let Some(lowest_common_ancestor) = lowest_common_ancestor {
            // Minimize the stream's end distance with the distance to the lowest common ancestor.
            self.end_distance = Some(std::cmp::min(
                self.end_distance.unwrap_or(u64::MAX),
                self.head.p1_linear_depth - lowest_common_ancestor.p1_linear_depth,
            ));
        }

        Ok(self)
    }

    /// Include only the linear descendants of the given changeset in the stream.
    pub async fn descendants_of(mut self, descendants_of: ChangesetId) -> Result<Self> {
        // Find the linear ancestor of head that is at the same level as `descendants_of`.
        let descendants_of = self
            .commit_graph
            .changeset_node(&self.ctx, descendants_of)
            .await?;
        let level_ancestor = self
            .commit_graph
            .p1_linear_level_ancestor(&self.ctx, self.head.cs_id, descendants_of.p1_linear_depth)
            .await?;

        match level_ancestor {
            // If the level ancestor is `descendants_of`, then minimize the stream's
            // end distance with the distance to the level ancestor.
            Some(level_ancestor) if level_ancestor.cs_id == descendants_of.cs_id => {
                self.end_distance = Some(std::cmp::min(
                    self.end_distance.unwrap_or(u64::MAX),
                    self.head.p1_linear_depth - level_ancestor.p1_linear_depth + 1,
                ));
            }
            // If the level ancestor isn't `descendants_of`, then `descendants_of` is
            // not an ancestor of `head` and the stream will be empty.
            _ => {
                self.end_distance = Some(self.start_distance);
            }
        }
        Ok(self)
    }

    /// Skip the first `skip` linear ancestors of the stream.
    pub fn skip(mut self, skip: u64) -> Self {
        self.start_distance = self.start_distance.saturating_add(skip);
        self
    }

    pub async fn build(self) -> Result<BoxStream<'static, Result<ChangesetId>>> {
        self.commit_graph
            .linear_ancestors_stream(
                self.ctx,
                self.head.cs_id,
                self.start_distance,
                self.end_distance,
            )
            .await
    }
}
