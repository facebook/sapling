/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::future::Future;
use std::sync::Arc;

use context::CoreContext;
use fbinit::FacebookInit;
use in_memory_commit_graph_storage::InMemoryCommitGraphStorage;

use super::*;

async fn run_test<Fut>(
    fb: FacebookInit,
    test_function: impl FnOnce(CoreContext, Arc<dyn CommitGraphStorageTest>) -> Fut,
) -> Result<()>
where
    Fut: Future<Output = Result<()>>,
{
    let ctx = CoreContext::test_mock(fb);
    let storage = Arc::new(InMemoryCommitGraphStorage::new(RepositoryId::new(1)));
    test_function(ctx, storage).await
}

impl_commit_graph_tests!(run_test);
