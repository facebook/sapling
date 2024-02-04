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

use crate::BufferedCommitGraphStorage;

impl CommitGraphStorageTest for BufferedCommitGraphStorage {}

async fn run_test<Fut>(
    fb: FacebookInit,
    test_function: impl FnOnce(CoreContext, Arc<dyn CommitGraphStorageTest>) -> Fut,
) -> Result<()>
where
    Fut: Future<Output = Result<()>>,
{
    let ctx = CoreContext::test_mock(fb);
    let storage = Arc::new(BufferedCommitGraphStorage::new(
        Arc::new(
            SqlCommitGraphStorageBuilder::with_sqlite_in_memory()
                .unwrap()
                .build(RendezVousOptions::for_test(), RepositoryId::new(1)),
        ),
        5,
    ));
    test_function(ctx, storage).await
}

impl_commit_graph_tests!(run_test);
