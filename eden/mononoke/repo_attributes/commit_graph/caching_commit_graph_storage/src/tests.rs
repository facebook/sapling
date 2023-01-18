/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use commit_graph_testlib::*;
use context::CoreContext;
use fbinit::FacebookInit;
use mononoke_types::RepositoryId;
use rendezvous::RendezVousOptions;
use sql_commit_graph_storage::SqlCommitGraphStorageBuilder;
use sql_construct::SqlConstruct;

use crate::CachingCommitGraphStorage;

#[fbinit::test]
async fn test_cached_sqlite_storage_store_and_fetch(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let storage = Arc::new(CachingCommitGraphStorage::mocked(Arc::new(
        SqlCommitGraphStorageBuilder::with_sqlite_in_memory()
            .unwrap()
            .build(RendezVousOptions::for_test(), RepositoryId::new(1)),
    )));

    test_storage_store_and_fetch(&ctx, storage.clone()).await?;
    assert!(storage.cachelib.mock_store().unwrap().stats().hits > 0);
    Ok(())
}

#[fbinit::test]
async fn test_cached_sqlite_skip_tree(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let storage = Arc::new(CachingCommitGraphStorage::mocked(Arc::new(
        SqlCommitGraphStorageBuilder::with_sqlite_in_memory()
            .unwrap()
            .build(RendezVousOptions::for_test(), RepositoryId::new(1)),
    )));

    test_skip_tree(&ctx, storage.clone()).await?;
    assert!(storage.cachelib.mock_store().unwrap().stats().hits > 0);
    Ok(())
}

#[fbinit::test]
async fn test_cached_sqlite_p1_linear_tree(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let storage = Arc::new(CachingCommitGraphStorage::mocked(Arc::new(
        SqlCommitGraphStorageBuilder::with_sqlite_in_memory()
            .unwrap()
            .build(RendezVousOptions::for_test(), RepositoryId::new(1)),
    )));

    test_p1_linear_tree(&ctx, storage.clone()).await?;
    assert!(storage.cachelib.mock_store().unwrap().stats().hits > 0);
    Ok(())
}

#[fbinit::test]
async fn test_cached_sqlite_get_ancestors_difference(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let storage = Arc::new(CachingCommitGraphStorage::mocked(Arc::new(
        SqlCommitGraphStorageBuilder::with_sqlite_in_memory()
            .unwrap()
            .build(RendezVousOptions::for_test(), RepositoryId::new(1)),
    )));

    test_get_ancestors_difference(&ctx, storage.clone()).await?;
    assert!(storage.cachelib.mock_store().unwrap().stats().hits > 0);
    Ok(())
}

#[fbinit::test]
async fn test_cached_sqlite_find_by_prefix(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let storage = Arc::new(CachingCommitGraphStorage::mocked(Arc::new(
        SqlCommitGraphStorageBuilder::with_sqlite_in_memory()
            .unwrap()
            .build(RendezVousOptions::for_test(), RepositoryId::new(1)),
    )));

    test_find_by_prefix(&ctx, storage.clone()).await?;
    assert!(storage.cachelib.mock_store().unwrap().stats().hits > 0);
    Ok(())
}

#[fbinit::test]
async fn test_cached_sqlite_add_recursive(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let storage = Arc::new(CachingCommitGraphStorage::mocked(Arc::new(
        SqlCommitGraphStorageBuilder::with_sqlite_in_memory()
            .unwrap()
            .build(RendezVousOptions::for_test(), RepositoryId::new(1)),
    )));

    test_add_recursive(&ctx, storage.clone()).await?;
    assert!(storage.cachelib.mock_store().unwrap().stats().hits > 0);
    Ok(())
}
