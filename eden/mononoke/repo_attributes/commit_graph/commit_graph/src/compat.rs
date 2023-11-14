/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use buffered_commit_graph_storage::BufferedCommitGraphStorage;
use changeset_fetcher::ArcChangesetFetcher;
use changeset_fetcher::ChangesetFetcher;
use commit_graph_types::edges::ChangesetEdges;
use commit_graph_types::edges::ChangesetParents;
use commit_graph_types::storage::CommitGraphStorage;
use commit_graph_types::storage::Prefetch;
use context::CoreContext;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;
use smallvec::SmallVec;
use smallvec::ToSmallVec;
use vec1::Vec1;

use crate::CommitGraph;

impl CommitGraph {
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
                        .maybe_fetch_many_edges(ctx, &parents_to_fetch, Prefetch::None)
                        .await
                        .with_context(|| "during commit_graph::add_recursive (fetch_many_edges)")?
                        .into_iter()
                        .map(|(k, v)| (k, v.into())),
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
}

#[async_trait]
impl ChangesetFetcher for CommitGraph {
    async fn get_generation_number(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Generation> {
        self.changeset_generation(ctx, cs_id).await
    }

    async fn get_parents(&self, ctx: &CoreContext, cs_id: ChangesetId) -> Result<Vec<ChangesetId>> {
        self.changeset_parents(ctx, cs_id)
            .await
            .map(SmallVec::into_vec)
    }
}
