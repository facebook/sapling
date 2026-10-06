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
use std::time::Duration;

use anyhow::Result;
use commit_graph_testlib::shuffling_storage::ShufflingCommitGraphStorage;
use commit_graph_testlib::utils::cs_id_name;
use commit_graph_testlib::utils::from_dag;
use commit_graph_testlib::utils::name_cs_id;
use commit_graph_testlib::utils::test_repo_identity;
use commit_graph_testlib::*;
use commit_graph_types::storage::CommitGraphStorage;
use commit_graph_types::storage::Prefetch;
use commit_graph_types::storage::PrefetchTarget;
use context::CoreContext;
use fbinit::FacebookInit;
use mononoke_macros::mononoke;
use mononoke_types::Generation;
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
            .build(RendezVousOptions::for_test(), test_repo_identity()),
    );
    test_function(ctx, storage).await
}

impl_commit_graph_tests!(run_test);

#[mononoke::fbinit_test]
pub async fn test_max_id_with_empty_graph(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let storage = Arc::new(
        SqlCommitGraphStorageBuilder::with_sqlite_in_memory()
            .unwrap()
            .build(RendezVousOptions::for_test(), test_repo_identity()),
    );
    let graph = from_dag(&ctx, r##""##, storage.clone()).await?;
    assert_eq!(storage.max_id(&ctx, false).await?, Some(0));
    Ok(())
}

#[mononoke::fbinit_test]
pub async fn test_lower_level_api(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let storage = Arc::new(
        SqlCommitGraphStorageBuilder::with_sqlite_in_memory()
            .unwrap()
            .build(RendezVousOptions::for_test(), test_repo_identity()),
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
            .await?
            .into_iter()
            .map(|(k, (_, v))| (k, v))
            .collect::<HashMap<_, _>>(),
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
            .await?
            .into_iter()
            .map(|(k, (_, v))| (k, v))
            .collect::<HashMap<_, _>>(),
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
            .await?
            .into_iter()
            .map(|(k, (_, v))| (k, v))
            .collect::<HashMap<_, _>>(),
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

#[mononoke::fbinit_test]
pub async fn test_exact_skip_tree_prefetch_crosses_skip_tree_roots(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let storage = Arc::new(
        SqlCommitGraphStorageBuilder::with_sqlite_in_memory()
            .unwrap()
            .build(RendezVousOptions::for_test(), test_repo_identity()),
    );

    // M merges two histories that have no common ancestor, which makes it a
    // skip tree root.  The skew binary traversal continues through the
    // parents of such roots, so prefetching its path must return them, but
    // it only follows the first parent further: W is never returned.
    from_dag(
        &ctx,
        r"
         A-B-M-N
            /
         W-X
         ",
        storage.clone(),
    )
    .await?;

    let prefetched_names = |generation: u64| {
        let storage = storage.clone();
        let ctx = ctx.clone();
        async move {
            let edges = storage
                .fetch_many_edges(
                    &ctx,
                    &[name_cs_id("N")],
                    Prefetch::Include(PrefetchTarget::ExactSkipTreeAncestors {
                        generation: Generation::new(generation),
                    }),
                )
                .await?;
            let mut names: Vec<_> = edges.keys().map(|cs_id| cs_id_name(*cs_id)).collect();
            names.sort();
            anyhow::Ok(names)
        }
    };

    assert_eq!(prefetched_names(1).await?, ["A", "B", "M", "N", "X"]);
    assert_eq!(prefetched_names(2).await?, ["B", "M", "N", "X"]);

    Ok(())
}

#[mononoke::fbinit_test]
pub async fn test_batched_prefetches_keep_their_targets(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    // No free connections and a long delay, so that concurrent requests are
    // batched together.
    let storage = Arc::new(
        SqlCommitGraphStorageBuilder::with_sqlite_in_memory()
            .unwrap()
            .build(
                RendezVousOptions {
                    free_connections: 0,
                    max_delay: Duration::from_millis(100),
                    max_threshold: 100,
                    cap_batch_at_threshold: true,
                },
                test_repo_identity(),
            ),
    );

    from_dag(&ctx, r"A-B-C-D-E-F-G-H", storage.clone()).await?;

    let prefetched_names = |name: &'static str, generation: u64| {
        let storage = storage.clone();
        let ctx = ctx.clone();
        async move {
            let edges = storage
                .fetch_many_edges(
                    &ctx,
                    &[name_cs_id(name)],
                    Prefetch::Include(PrefetchTarget::LinearAncestors {
                        generation: Generation::new(generation),
                        steps: 10,
                    }),
                )
                .await?;
            let mut names: Vec<_> = edges.keys().map(|cs_id| cs_id_name(*cs_id)).collect();
            names.sort();
            anyhow::Ok(names)
        }
    };

    // Each request's prefetch stops at its own generation rather than at
    // the generation of the request that opened the batch.
    let (from_h, from_d) = futures::try_join!(prefetched_names("H", 6), prefetched_names("D", 1))?;
    assert_eq!(from_h, ["F", "G", "H"]);
    assert_eq!(from_d, ["A", "B", "C", "D"]);

    Ok(())
}
