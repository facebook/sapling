/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;

use anyhow::anyhow;
use anyhow::Result;
use async_trait::async_trait;
use commit_graph::edges::ChangesetEdges;
use commit_graph::storage::CommitGraphStorage;
use context::CoreContext;
use mononoke_types::ChangesetId;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::Generation;
use mononoke_types::RepositoryId;
use parking_lot::RwLock;
use vec1::Vec1;

/// In-memory commit graph storage.
pub struct InMemoryCommitGraphStorage {
    repo_id: RepositoryId,
    changesets: RwLock<BTreeMap<ChangesetId, ChangesetEdges>>,
}

impl InMemoryCommitGraphStorage {
    pub fn new(repo_id: RepositoryId) -> Self {
        InMemoryCommitGraphStorage {
            repo_id,
            changesets: Default::default(),
        }
    }

    pub fn drain(&self) -> Vec<ChangesetEdges> {
        let mut changesets = self.changesets.write();
        let many_edges = changesets.iter().map(|(_, edges)| edges).cloned().collect();
        changesets.clear();
        many_edges
    }

    pub fn len(&self) -> usize {
        self.changesets.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.changesets.read().is_empty()
    }
}

#[async_trait]
impl CommitGraphStorage for InMemoryCommitGraphStorage {
    fn repo_id(&self) -> RepositoryId {
        self.repo_id
    }

    async fn add(&self, _ctx: &CoreContext, edges: ChangesetEdges) -> Result<bool> {
        let cs_id = edges.node.cs_id;
        Ok(self.changesets.write().insert(cs_id, edges).is_none())
    }

    async fn add_many(
        &self,
        _ctx: &CoreContext,
        many_edges: Vec1<ChangesetEdges>,
    ) -> Result<usize> {
        let mut changesets = self.changesets.write();
        let mut added = 0;
        for edges in many_edges {
            if changesets.insert(edges.node.cs_id, edges).is_none() {
                added += 1;
            }
        }
        Ok(added)
    }

    async fn fetch_edges(
        &self,
        _ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Option<ChangesetEdges>> {
        Ok(self.changesets.read().get(&cs_id).cloned())
    }

    async fn fetch_edges_required(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<ChangesetEdges> {
        self.fetch_edges(ctx, cs_id).await?.ok_or_else(|| {
            anyhow!(
                "Missing changeset from in-memory commit graph storage: {}",
                cs_id
            )
        })
    }

    async fn fetch_many_edges(
        &self,
        _ctx: &CoreContext,
        cs_ids: &[ChangesetId],
        _prefetch_hint: Option<Generation>,
    ) -> Result<HashMap<ChangesetId, ChangesetEdges>> {
        let mut result = HashMap::with_capacity(cs_ids.len());
        let changesets = self.changesets.read();
        for cs_id in cs_ids {
            if let Some(edges) = changesets.get(cs_id) {
                result.insert(*cs_id, edges.clone());
            }
        }
        Ok(result)
    }

    async fn fetch_many_edges_required(
        &self,
        ctx: &CoreContext,
        cs_ids: &[ChangesetId],
        prefetch_hint: Option<Generation>,
    ) -> Result<HashMap<ChangesetId, ChangesetEdges>> {
        let edges = self.fetch_many_edges(ctx, cs_ids, prefetch_hint).await?;
        let missing_changesets: Vec<_> = cs_ids
            .iter()
            .filter(|cs_id| !edges.contains_key(cs_id))
            .collect();

        if !missing_changesets.is_empty() {
            Err(anyhow!(
                "Missing changesets from in-memory commit graph storage: {}",
                missing_changesets
                    .into_iter()
                    .map(|cs_id| format!("{}, ", cs_id))
                    .collect::<String>()
            ))
        } else {
            Ok(edges)
        }
    }

    async fn find_by_prefix(
        &self,
        _ctx: &CoreContext,
        cs_prefix: ChangesetIdPrefix,
        limit: usize,
    ) -> Result<ChangesetIdsResolvedFromPrefix> {
        let changesets = self.changesets.read();
        let (min, max) = (cs_prefix.min_bound(), cs_prefix.max_bound());
        let matches: Vec<_> = changesets
            .range(min..=max)
            .take(limit.saturating_add(1))
            .map(|(cs_id, _)| *cs_id)
            .collect();
        Ok(ChangesetIdsResolvedFromPrefix::from_vec_and_limit(
            matches, limit,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use commit_graph_testlib::*;
    use context::CoreContext;
    use fbinit::FacebookInit;

    use super::*;

    #[fbinit::test]
    async fn test_in_memory_storage_store_and_fetch(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let storage = Arc::new(InMemoryCommitGraphStorage::new(RepositoryId::new(1)));

        test_storage_store_and_fetch(&ctx, storage).await
    }

    #[fbinit::test]
    async fn test_in_memory_skip_tree(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let storage = Arc::new(InMemoryCommitGraphStorage::new(RepositoryId::new(1)));

        test_skip_tree(&ctx, storage).await
    }

    #[fbinit::test]
    async fn test_in_memory_p1_linear_tree(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let storage = Arc::new(InMemoryCommitGraphStorage::new(RepositoryId::new(1)));

        test_p1_linear_tree(&ctx, storage).await
    }

    #[fbinit::test]
    async fn test_in_memory_get_ancestors_difference(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let storage = Arc::new(InMemoryCommitGraphStorage::new(RepositoryId::new(1)));

        test_get_ancestors_difference(&ctx, storage).await
    }

    #[fbinit::test]
    async fn test_in_memory_find_by_prefix(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let storage = Arc::new(InMemoryCommitGraphStorage::new(RepositoryId::new(1)));

        test_find_by_prefix(&ctx, storage).await
    }

    #[fbinit::test]
    async fn test_in_memory_add_recursive(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let storage = Arc::new(InMemoryCommitGraphStorage::new(RepositoryId::new(1)));

        test_add_recursive(&ctx, storage).await
    }
}
