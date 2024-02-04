/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::future::Future;
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

impl CommitGraphStorageTest for CachingCommitGraphStorage {
    fn flush(&self) {
        if let Some(mock) = self.memcache.mock_store() {
            mock.flush();
        }
        if let Some(mock) = self.cachelib.mock_store() {
            mock.flush();
        }
    }
}

async fn run_test<Fut>(
    fb: FacebookInit,
    test_function: impl FnOnce(CoreContext, Arc<dyn CommitGraphStorageTest>) -> Fut,
) -> Result<()>
where
    Fut: Future<Output = Result<()>>,
{
    let ctx = CoreContext::test_mock(fb);
    let storage = Arc::new(CachingCommitGraphStorage::mocked(Arc::new(
        SqlCommitGraphStorageBuilder::with_sqlite_in_memory()
            .unwrap()
            .build(RendezVousOptions::for_test(), RepositoryId::new(1)),
    )));
    test_function(ctx, storage.clone()).await?;
    assert!(storage.cachelib.mock_store().unwrap().stats().hits > 0);
    Ok(())
}

impl_commit_graph_tests!(run_test);
