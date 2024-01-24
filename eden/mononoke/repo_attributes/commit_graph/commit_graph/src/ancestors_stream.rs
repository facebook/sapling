/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::pin::Pin;

use anyhow::Result;
use borrowed::borrowed;
use commit_graph_types::frontier::ChangesetFrontier;
use commit_graph_types::storage::Prefetch;
use context::CoreContext;
use futures::future;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::Future;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;

use crate::ArcCommitGraph;
use crate::CommitGraph;

/// Builder for a reverse topologically ordered stream of changesets that
/// are ancestors of any set of changesets (heads). This builder allows customizing
/// the stream by:
///
/// - excluding ancestors of a set of changesets (common).
///
/// - excluding changesets that satisify a given property (if this property holds
/// for one changeset then it has to hold for all its parents).
///
/// - including only changesets that satisfy a given property (if this property doesn't
/// hold for one changeset then it mustn't hold for any of its parents).
///
/// - including only changesets that are descendants of any one changeset.
pub struct AncestorsStreamBuilder {
    commit_graph: ArcCommitGraph,
    ctx: CoreContext,
    heads: Vec<ChangesetId>,
    common: Vec<ChangesetId>,
    descendants_of: Option<ChangesetId>,
    property: Box<
        dyn Fn(ChangesetId) -> Pin<Box<dyn Future<Output = Result<bool>> + Send>> + Send + Sync,
    >,
}

impl AncestorsStreamBuilder {
    pub fn new(commit_graph: ArcCommitGraph, ctx: CoreContext, heads: Vec<ChangesetId>) -> Self {
        Self {
            commit_graph,
            ctx,
            heads,
            common: vec![],
            descendants_of: None,
            property: Box::new(|_| Box::pin(future::ready(Ok(true)))),
        }
    }

    pub fn exclude_ancestors_of(mut self, common: Vec<ChangesetId>) -> Self {
        self.common.extend(common);
        self
    }

    pub fn descendants_of(mut self, descendants_of: ChangesetId) -> Self {
        self.descendants_of = Some(descendants_of);
        self
    }

    pub fn with<Property, Out>(mut self, other_property: Property) -> Self
    where
        Property: Fn(ChangesetId) -> Out + Send + Sync + 'static,
        Out: Future<Output = Result<bool>> + Send + 'static,
    {
        self.property = Box::new(move |cs_id| {
            let fut_property = (self.property)(cs_id);
            let fut_other_property = other_property(cs_id);

            Box::pin(async move {
                if !fut_property.await? {
                    Ok(false)
                } else {
                    fut_other_property.await
                }
            })
        });
        self
    }

    pub fn without<Property, Out>(mut self, other_property: Property) -> Self
    where
        Property: Fn(ChangesetId) -> Out + Send + Sync + 'static,
        Out: Future<Output = Result<bool>> + Send + 'static,
    {
        self.property = Box::new(move |cs_id| {
            let fut_property = (self.property)(cs_id);
            let fut_other_property = other_property(cs_id);

            Box::pin(async move {
                if !fut_property.await? {
                    Ok(false)
                } else {
                    Ok(!fut_other_property.await?)
                }
            })
        });
        self
    }

    pub async fn build(self) -> Result<BoxStream<'static, Result<ChangesetId>>> {
        struct AncestorsStreamState {
            commit_graph: ArcCommitGraph,
            ctx: CoreContext,
            heads: ChangesetFrontier,
            common: ChangesetFrontier,
            descendants_of: Option<(ChangesetId, Generation)>,
            property: Box<
                dyn Fn(ChangesetId) -> Pin<Box<dyn Future<Output = Result<bool>> + Send>>
                    + Send
                    + Sync,
            >,
        }

        let heads = match self.descendants_of {
            Some(descendants_of) => {
                stream::iter(self.heads)
                    .map(anyhow::Ok)
                    .try_filter_map(|head| {
                        borrowed!(self.commit_graph: &CommitGraph, self.ctx);
                        async move {
                            match commit_graph.is_ancestor(ctx, descendants_of, head).await? {
                                true => Ok(Some(head)),
                                false => Ok(None),
                            }
                        }
                    })
                    .try_collect()
                    .await?
            }
            None => self.heads,
        };

        let descendants_of = match self.descendants_of {
            Some(descendants_of) => Some((
                descendants_of,
                self.commit_graph
                    .changeset_generation(&self.ctx, descendants_of)
                    .await?,
            )),
            None => None,
        };

        let (heads, common) = futures::try_join!(
            self.commit_graph.frontier(&self.ctx, heads),
            self.commit_graph.frontier(&self.ctx, self.common)
        )?;

        Ok(stream::try_unfold(
            Box::new(AncestorsStreamState {
                commit_graph: self.commit_graph,
                ctx: self.ctx,
                heads,
                common,
                descendants_of,
                property: self.property,
            }),
            move |mut state| async move {
                let AncestorsStreamState {
                    commit_graph,
                    ctx,
                    heads,
                    common,
                    descendants_of,
                    property,
                } = &mut *state;

                if let Some((generation, cs_ids)) = heads.pop_last() {
                    commit_graph.lower_frontier(ctx, common, generation).await?;

                    let mut cs_ids_not_excluded = vec![];
                    for cs_id in cs_ids {
                        if !common.highest_generation_contains(cs_id, generation)
                            && property(cs_id).await?
                        {
                            cs_ids_not_excluded.push(cs_id)
                        }
                    }

                    let all_edges = commit_graph
                        .storage
                        .fetch_many_edges(
                            ctx,
                            &cs_ids_not_excluded,
                            Prefetch::for_p1_linear_traversal(),
                        )
                        .await?;

                    for (_cs_id, edges) in all_edges.into_iter() {
                        for parent in edges.parents.iter() {
                            if let Some((descendants_of, descendants_of_gen)) = descendants_of {
                                // There is no need to query ancestry if the skip tree parent's generation number
                                // is greater than or equal to the generation number of descendants_of. This is
                                // because the skip tree parent is the common ancestor of all parents, and since
                                // the current changeset is a descendant of descendants_of, all of its parents
                                // will also be descendants of it.
                                if !edges.skip_tree_parent.map_or(false, |skip_tree_parent| {
                                    skip_tree_parent.generation >= *descendants_of_gen
                                }) && !commit_graph
                                    .is_ancestor(ctx, *descendants_of, parent.cs_id)
                                    .await?
                                {
                                    continue;
                                }
                            }
                            heads
                                .entry(parent.generation)
                                .or_default()
                                .insert(parent.cs_id);
                        }
                    }

                    anyhow::Ok(Some((stream::iter(cs_ids_not_excluded).map(Ok), state)))
                } else {
                    Ok(None)
                }
            },
        )
        .try_flatten()
        .boxed())
    }
}
