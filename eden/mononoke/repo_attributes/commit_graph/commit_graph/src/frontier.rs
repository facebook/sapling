/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Result;
use borrowed::borrowed;
use commit_graph_types::edges::ChangesetNode;
use commit_graph_types::frontier::ChangesetFrontier;
use commit_graph_types::storage::Prefetch;
use context::CoreContext;
use futures::future;
use futures::Future;
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
        let generation = self.changeset_generation(ctx, cs_id).await?;
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

        cs_ids
            .into_iter()
            .map(|cs_id| {
                Ok((
                    cs_id,
                    all_edges
                        .get(&cs_id)
                        .ok_or_else(|| anyhow!("Missing changeset in commit graph: {}", cs_id))?
                        .node
                        .generation,
                ))
            })
            .collect::<Result<_>>()
    }

    /// Pops the highest generation changesets of a frontier, returning any that
    /// satisify a property and lowering the rest of them to either their immediate
    /// parents or their lowest skip tree edge that doesn't satisfy the property.
    /// Repeatedly calling this function and concatenating the output will result
    /// in the frontier of changesets satisifying the property.
    ///
    /// Returns None if the frontier is empty.
    pub(crate) async fn lower_frontier_step<Property, Out>(
        &self,
        ctx: &CoreContext,
        frontier: &mut ChangesetFrontier,
        property: Property,
        prefetch: Prefetch,
    ) -> Result<Option<Vec<ChangesetId>>>
    where
        Property: Fn(ChangesetNode) -> Out + Send + Sync,
        Out: Future<Output = Result<bool>>,
    {
        match frontier.pop_last() {
            None => Ok(None),
            Some((_, cs_ids)) => {
                let cs_ids = cs_ids.into_iter().collect::<Vec<_>>();
                let frontier_edges = self
                    .storage
                    .fetch_many_edges(ctx, &cs_ids, prefetch)
                    .await?;

                let mut property_frontier: Vec<_> = Default::default();

                for (_, edges) in frontier_edges {
                    if property(edges.node).await? {
                        property_frontier.push(edges.node.cs_id);
                    } else {
                        match edges
                            .lowest_skip_tree_edge_with(|node| {
                                borrowed!(property);
                                async move { Ok(!property(node).await?) }
                            })
                            .await?
                        {
                            Some(ancestor) => {
                                frontier
                                    .entry(ancestor.generation)
                                    .or_default()
                                    .insert(ancestor.cs_id);
                            }
                            None => {
                                for parent in &edges.parents {
                                    frontier
                                        .entry(parent.generation)
                                        .or_default()
                                        .insert(parent.cs_id);
                                }
                            }
                        }
                    }
                }

                Ok(Some(property_frontier))
            }
        }
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

            self.lower_frontier_step(
                ctx,
                frontier,
                move |node| future::ready(Ok(node.generation < target_generation)),
                Prefetch::for_skip_tree_traversal(target_generation),
            )
            .await?;
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
                .fetch_many_edges(ctx, &cs_ids, Prefetch::for_p1_linear_traversal())
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

    /// Minimize a frontier by removing all changesets that are ancestors of other changesets
    /// in the frontier.
    pub async fn minimize_frontier(
        &self,
        ctx: &CoreContext,
        frontier: Vec<ChangesetId>,
    ) -> Result<Vec<ChangesetId>> {
        // Process the frontier generation by generation starting from the highest,
        // removing changesets that are ancestors of a higher generation changeset.

        let mut processed_frontier = ChangesetFrontier::new();
        let mut remaining_frontier = self.frontier(ctx, frontier).await?;
        let mut minimal_frontier = vec![];

        while let Some((generation, cs_ids)) = remaining_frontier.pop_last() {
            // Lower the frontier of the previously processed generations to the current
            // generation. Any changeset that's contained in this frontier is an ancestor
            // of a higher generation changeset and should be removed.
            self.lower_frontier(ctx, &mut processed_frontier, generation)
                .await?;

            let new_cs_ids = cs_ids
                .iter()
                .copied()
                .filter(|cs_id| !processed_frontier.highest_generation_contains(*cs_id, generation))
                .collect::<Vec<_>>();

            minimal_frontier.extend(new_cs_ids.clone());
            processed_frontier.extend(new_cs_ids.into_iter().map(|cs_id| (cs_id, generation)))
        }

        Ok(minimal_frontier)
    }
}
