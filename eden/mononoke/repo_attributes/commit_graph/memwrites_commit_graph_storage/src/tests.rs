/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::future::Future;
use std::sync::Arc;

use anyhow::Result;
use commit_graph_testlib::utils::from_dag;
use commit_graph_testlib::utils::name_cs_id;
use commit_graph_testlib::utils::test_repo_identity;
use commit_graph_testlib::*;
use commit_graph_types::storage::CommitGraphStorage;
use context::CoreContext;
use fbinit::FacebookInit;
use in_memory_commit_graph_storage::InMemoryCommitGraphStorage;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use rendezvous::RendezVousOptions;
use sql_commit_graph_storage::SqlCommitGraphStorageBuilder;
use sql_construct::SqlConstruct;

use crate::MemWritesCommitGraphStorage;

impl CommitGraphStorageTest for MemWritesCommitGraphStorage {}

async fn run_test<Fut>(
    fb: FacebookInit,
    test_function: impl FnOnce(CoreContext, Arc<dyn CommitGraphStorageTest>) -> Fut,
) -> Result<()>
where
    Fut: Future<Output = Result<()>>,
{
    let ctx = CoreContext::test_mock(fb);
    let storage = Arc::new(MemWritesCommitGraphStorage::new(Arc::new(
        SqlCommitGraphStorageBuilder::with_sqlite_in_memory()
            .unwrap()
            .build(RendezVousOptions::for_test(), test_repo_identity()),
    )));
    test_function(ctx, storage).await
}

impl_commit_graph_tests!(run_test);

#[mononoke::fbinit_test]
async fn test_find_by_prefix_dedups_changeset_in_both_layers(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let persistent = Arc::new(InMemoryCommitGraphStorage::new(test_repo_identity()));
    from_dag(&ctx, r"A-B", persistent.clone()).await?;

    // Add B, which the persistent storage already has, through the in-memory layer.
    let storage = MemWritesCommitGraphStorage::new(persistent.clone());
    let b = name_cs_id("B");
    storage
        .add(&ctx, persistent.fetch_edges(&ctx, b).await?)
        .await?;

    let prefix = ChangesetIdPrefix::from_bytes(b.as_ref())?;
    let resolved = storage.find_by_prefix(&ctx, prefix, 10).await?;
    assert_eq!(resolved, ChangesetIdsResolvedFromPrefix::Single(b));
    Ok(())
}
