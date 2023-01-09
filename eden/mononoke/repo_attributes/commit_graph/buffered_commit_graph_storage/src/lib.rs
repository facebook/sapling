/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;
use commit_graph::edges::ChangesetEdges;
use commit_graph::storage::CommitGraphStorage;
use context::CoreContext;
use in_memory_commit_graph_storage::InMemoryCommitGraphStorage;
use mononoke_types::ChangesetId;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::Generation;
use mononoke_types::RepositoryId;
use vec1::Vec1;

#[cfg(test)]
mod tests;

pub struct BufferedCommitGraphStorage<T: CommitGraphStorage> {
    in_memory_storage: InMemoryCommitGraphStorage,
    persistent_storage: T,
    /// The maximum number of changeset edges
    /// allowed to be stored in memory.
    max_in_memory_size: usize,
}

impl<T: CommitGraphStorage> BufferedCommitGraphStorage<T> {
    pub fn new(persistent_storage: T, max_in_memory_size: usize) -> Self {
        Self {
            in_memory_storage: InMemoryCommitGraphStorage::new(persistent_storage.repo_id()),
            persistent_storage,
            max_in_memory_size,
        }
    }

    /// Flushes all changeset edges from the in memory storage
    /// to the persistent storage. Returns the number of added
    /// edges to the persistent storage.
    pub async fn flush(&self, ctx: &CoreContext) -> Result<usize> {
        match Vec1::try_from_vec(self.in_memory_storage.drain()) {
            Ok(many_edges) => self.persistent_storage.add_many(ctx, many_edges).await,
            _ => Ok(0),
        }
    }
}

#[async_trait]
impl<T: CommitGraphStorage> CommitGraphStorage for BufferedCommitGraphStorage<T> {
    fn repo_id(&self) -> RepositoryId {
        self.persistent_storage.repo_id()
    }

    async fn add(&self, ctx: &CoreContext, edges: ChangesetEdges) -> Result<bool> {
        if self.in_memory_storage.len() + 1 > self.max_in_memory_size {
            self.flush(ctx).await?;
        }
        self.in_memory_storage.add(ctx, edges).await
    }

    async fn add_many(&self, ctx: &CoreContext, many_edges: Vec1<ChangesetEdges>) -> Result<usize> {
        if self.in_memory_storage.len() + many_edges.len() > self.max_in_memory_size {
            self.flush(ctx).await?;
        }
        self.in_memory_storage.add_many(ctx, many_edges).await
    }

    async fn fetch_edges(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Option<ChangesetEdges>> {
        match self.in_memory_storage.fetch_edges(ctx, cs_id).await? {
            Some(edges) => Ok(Some(edges)),
            None => self.persistent_storage.fetch_edges(ctx, cs_id).await,
        }
    }

    async fn fetch_edges_required(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<ChangesetEdges> {
        match self.in_memory_storage.fetch_edges(ctx, cs_id).await? {
            Some(edges) => Ok(edges),
            None => {
                self.persistent_storage
                    .fetch_edges_required(ctx, cs_id)
                    .await
            }
        }
    }

    async fn fetch_many_edges(
        &self,
        ctx: &CoreContext,
        cs_ids: &[ChangesetId],
        prefetch_hint: Option<Generation>,
    ) -> Result<HashMap<ChangesetId, ChangesetEdges>> {
        let mut fetched_edges = self
            .in_memory_storage
            .fetch_many_edges(ctx, cs_ids, prefetch_hint)
            .await?;

        let unfetched_ids = cs_ids
            .iter()
            .filter(|cs_id| !fetched_edges.contains_key(cs_id))
            .copied()
            .collect::<Vec<_>>();

        if !unfetched_ids.is_empty() {
            fetched_edges.extend(
                self.persistent_storage
                    .fetch_many_edges(ctx, unfetched_ids.as_slice(), prefetch_hint)
                    .await?
                    .into_iter(),
            )
        }

        Ok(fetched_edges)
    }

    async fn fetch_many_edges_required(
        &self,
        ctx: &CoreContext,
        cs_ids: &[ChangesetId],
        prefetch_hint: Option<Generation>,
    ) -> Result<HashMap<ChangesetId, ChangesetEdges>> {
        let mut fetched_edges = self
            .in_memory_storage
            .fetch_many_edges(ctx, cs_ids, prefetch_hint)
            .await?;

        let unfetched_ids = cs_ids
            .iter()
            .filter(|cs_id| !fetched_edges.contains_key(cs_id))
            .copied()
            .collect::<Vec<_>>();

        if !unfetched_ids.is_empty() {
            fetched_edges.extend(
                self.persistent_storage
                    .fetch_many_edges_required(ctx, unfetched_ids.as_slice(), prefetch_hint)
                    .await?
                    .into_iter(),
            )
        }

        Ok(fetched_edges)
    }

    async fn find_by_prefix(
        &self,
        ctx: &CoreContext,
        cs_prefix: ChangesetIdPrefix,
        limit: usize,
    ) -> Result<ChangesetIdsResolvedFromPrefix> {
        match futures::try_join!(
            self.in_memory_storage.find_by_prefix(ctx, cs_prefix, limit),
            self.persistent_storage
                .find_by_prefix(ctx, cs_prefix, limit)
        )? {
            (in_memory_matches @ ChangesetIdsResolvedFromPrefix::TooMany(_), _) => {
                Ok(in_memory_matches)
            }
            (_, persistent_matches @ ChangesetIdsResolvedFromPrefix::TooMany(_)) => {
                Ok(persistent_matches)
            }
            (in_memory_matches, persistent_matches) => {
                Ok(ChangesetIdsResolvedFromPrefix::from_vec_and_limit(
                    in_memory_matches
                        .to_vec()
                        .into_iter()
                        .chain(persistent_matches.to_vec().into_iter())
                        .collect(),
                    limit,
                ))
            }
        }
    }
}
