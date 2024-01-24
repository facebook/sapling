/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;

use anyhow::Result;
use commit_graph_testlib::utils::from_dag;
use commit_graph_testlib::utils::name_cs_id;
use commit_graph_testlib::*;
use commit_graph_types::storage::CommitGraphStorage;
use commit_graph_types::storage::Prefetch;
use context::CoreContext;
use fbinit::FacebookInit;
use mononoke_types::RepositoryId;
use rendezvous::RendezVousOptions;
use sql_construct::SqlConstruct;

use crate::SqlCommitGraphStorage;
use crate::SqlCommitGraphStorageBuilder;

impl CommitGraphStorageTest for SqlCommitGraphStorage {}

async fn run_test<Fut>(
    fb: FacebookInit,
    test_function: impl FnOnce(CoreContext, Arc<dyn CommitGraphStorageTest>) -> Fut,
) -> Result<()>
where
    Fut: Future<Output = Result<()>>,
{
    let ctx = CoreContext::test_mock(fb);
    let storage = Arc::new(
        SqlCommitGraphStorageBuilder::with_sqlite_in_memory()
            .unwrap()
            .build(RendezVousOptions::for_test(), RepositoryId::new(1)),
    );
    test_function(ctx, storage).await
}

impl_commit_graph_tests!(run_test);

#[fbinit::test]
pub async fn test_lower_level_api(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let storage = Arc::new(
        SqlCommitGraphStorageBuilder::with_sqlite_in_memory()
            .unwrap()
            .build(RendezVousOptions::for_test(), RepositoryId::new(1)),
    );

    let graph = from_dag(
        &ctx,
        r##"
             A-B-C-D-E-F-G-H-I-J
         "##,
        storage.clone(),
    )
    .await?;

    assert_eq!(storage.max_id(&ctx, false).await?, Some(10));
    assert_eq!(storage.max_id(&ctx, true).await?, Some(10));

    assert_eq!(
        storage.max_id_in_range(&ctx, 1, 10, 10, false).await?,
        Some(10),
    );
    assert_eq!(
        storage.max_id_in_range(&ctx, 2, 10, 5, false).await?,
        Some(6),
    );
    assert_eq!(
        storage.max_id_in_range(&ctx, 4, 7, 100, false).await?,
        Some(7),
    );

    assert_eq!(
        storage
            .fetch_many_cs_ids_in_id_range(&ctx, 1, 10, 10, false)
            .await?,
        ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"]
            .into_iter()
            .map(name_cs_id)
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        storage
            .fetch_many_cs_ids_in_id_range(&ctx, 1, 10, 5, false)
            .await?,
        ["A", "B", "C", "D", "E"]
            .into_iter()
            .map(name_cs_id)
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        storage
            .fetch_many_cs_ids_in_id_range(&ctx, 4, 6, 100, false)
            .await?,
        ["D", "E", "F"]
            .into_iter()
            .map(name_cs_id)
            .collect::<Vec<_>>(),
    );

    let all_edges = storage
        .fetch_many_edges(
            &ctx,
            &["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"]
                .into_iter()
                .map(name_cs_id)
                .collect::<Vec<_>>(),
            Prefetch::None,
        )
        .await?;

    assert_eq!(
        storage
            .fetch_many_edges_in_id_range(&ctx, 1, 10, 10, false)
            .await?,
        ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"]
            .into_iter()
            .map(|id| {
                let cs_id = name_cs_id(id);
                (cs_id, all_edges.get(&cs_id).unwrap().clone().into())
            })
            .collect::<HashMap<_, _>>(),
    );
    assert_eq!(
        storage
            .fetch_many_edges_in_id_range(&ctx, 1, 10, 5, false)
            .await?,
        ["A", "B", "C", "D", "E"]
            .into_iter()
            .map(|id| {
                let cs_id = name_cs_id(id);
                (cs_id, all_edges.get(&cs_id).unwrap().clone().into())
            })
            .collect::<HashMap<_, _>>(),
    );
    assert_eq!(
        storage
            .fetch_many_edges_in_id_range(&ctx, 4, 6, 100, false)
            .await?,
        ["D", "E", "F"]
            .into_iter()
            .map(|id| {
                let cs_id = name_cs_id(id);
                (cs_id, all_edges.get(&cs_id).unwrap().clone().into())
            })
            .collect::<HashMap<_, _>>(),
    );

    Ok(())
}
