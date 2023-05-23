/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Result;
use commit_graph_types::frontier::ChangesetFrontier;
use commit_graph_types::storage::Prefetch;
use context::CoreContext;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;

use crate::CommitGraph;

impl CommitGraph {
    /// Obtain a frontier of changesets from a single changeset id, which must
    /// exist.
    pub(crate) async fn single_frontier(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<ChangesetFrontier> {
        let generation = self.changeset_generation_required(ctx, cs_id).await?;
        Ok(ChangesetFrontier::new_single(cs_id, generation))
    }

    /// Obtain a frontier of changesets from a list of changeset ids, which
    /// must all exist.
    pub(crate) async fn frontier(
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
    pub(crate) async fn lower_frontier(
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
    pub(crate) async fn lower_frontier_highest_generation(
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
}
