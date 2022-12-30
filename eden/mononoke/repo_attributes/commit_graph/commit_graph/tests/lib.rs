/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use anyhow::Result;
    use commit_graph::storage::InMemoryCommitGraphStorage;
    use commit_graph_testlib::*;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use mononoke_types::RepositoryId;

    #[fbinit::test]
    pub async fn test_in_memory_storage_store_and_fetch(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let storage = Arc::new(InMemoryCommitGraphStorage::new(RepositoryId::new(1)));

        test_storage_store_and_fetch(&ctx, storage).await
    }

    #[fbinit::test]
    pub async fn test_in_memory_skip_tree(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let storage = Arc::new(InMemoryCommitGraphStorage::new(RepositoryId::new(1)));

        test_skip_tree(&ctx, storage).await
    }

    #[fbinit::test]
    pub async fn test_in_memory_p1_linear_tree(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let storage = Arc::new(InMemoryCommitGraphStorage::new(RepositoryId::new(1)));

        test_p1_linear_tree(&ctx, storage).await
    }

    #[fbinit::test]
    pub async fn test_in_memory_get_ancestors_difference(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let storage = Arc::new(InMemoryCommitGraphStorage::new(RepositoryId::new(1)));

        test_get_ancestors_difference(&ctx, storage).await
    }

    #[fbinit::test]
    pub async fn test_in_memory_find_by_prefix(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let storage = Arc::new(InMemoryCommitGraphStorage::new(RepositoryId::new(1)));

        test_find_by_prefix(&ctx, storage).await
    }
}
