/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use commit_graph::CommitGraph;
use commit_graph_testlib::utils::from_dag;
use commit_graph_testlib::utils::name_cs_id;
use commit_graph_types::edges::ChangesetEdges;
use commit_graph_types::storage::CommitGraphStorage;
use commit_graph_types::storage::Prefetch;
use context::CoreContext;
use fbinit::FacebookInit;
use fbthrift::compact_protocol;
use in_memory_commit_graph_storage::InMemoryCommitGraphStorage;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use reloader::Reloader;

use crate::deserialize_preloaded_edges;
use crate::ExtendablePreloadedEdges;
use crate::PreloadedCommitGraphStorage;

impl PreloadedCommitGraphStorage {
    /// Constructs PreloadedCommitGraphStorage from a fixed list of edges
    /// for unit tests.
    pub fn from_edges(
        repo_id: RepositoryId,
        edges_vec: Vec<ChangesetEdges>,
        persistent_storage: Arc<dyn CommitGraphStorage>,
    ) -> Result<Arc<Self>> {
        let mut extendable_preloaded_edges: ExtendablePreloadedEdges = Default::default();

        for edges in edges_vec {
            extendable_preloaded_edges.add(edges)?;
        }

        // Serialize the preloaded edges and then deserialize,
        // to make sure no information is lost.
        let bytes = compact_protocol::serialize(
            &extendable_preloaded_edges
                .into_preloaded_edges()
                .to_thrift()?,
        );
        let preloaded_edges = deserialize_preloaded_edges(bytes)?;

        Ok(Arc::new(Self {
            repo_id,
            preloaded_edges: Reloader::fixed(preloaded_edges),
            persistent_storage,
        }))
    }
}

async fn test_equivalent_storages(
    ctx: &CoreContext,
    first_storage: Arc<dyn CommitGraphStorage>,
    second_storage: Arc<dyn CommitGraphStorage>,
    cs_ids: Vec<ChangesetId>,
) -> Result<()> {
    for cs_id in cs_ids.iter() {
        assert_eq!(
            first_storage.maybe_fetch_edges(ctx, *cs_id).await?,
            second_storage.maybe_fetch_edges(ctx, *cs_id).await?,
        );
        assert_eq!(
            first_storage.fetch_edges(ctx, *cs_id).await?,
            second_storage.fetch_edges(ctx, *cs_id).await?,
        );
    }

    assert_eq!(
        first_storage
            .maybe_fetch_many_edges(ctx, &cs_ids, Prefetch::None)
            .await?,
        second_storage
            .maybe_fetch_many_edges(ctx, &cs_ids, Prefetch::None)
            .await?
    );
    assert_eq!(
        first_storage
            .fetch_many_edges(ctx, &cs_ids, Prefetch::None)
            .await?,
        second_storage
            .fetch_many_edges(ctx, &cs_ids, Prefetch::None)
            .await?
    );

    Ok(())
}

#[fbinit::test]
async fn test_preloaded_commit_graph_storage(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let underlying_storage = Arc::new(InMemoryCommitGraphStorage::new(RepositoryId::new(1)));
    let _ = from_dag(
        &ctx,
        r"
            A-B-C-D-G-H-I
             \     /
              E---F
        ",
        underlying_storage.clone(),
    )
    .await?;

    let edges_vec = vec![
        underlying_storage
            .fetch_edges(&ctx, name_cs_id("A"))
            .await?,
        underlying_storage
            .fetch_edges(&ctx, name_cs_id("B"))
            .await?,
        underlying_storage
            .fetch_edges(&ctx, name_cs_id("C"))
            .await?,
        underlying_storage
            .fetch_edges(&ctx, name_cs_id("D"))
            .await?,
        underlying_storage
            .fetch_edges(&ctx, name_cs_id("E"))
            .await?,
    ];

    let preloaded_storage = PreloadedCommitGraphStorage::from_edges(
        RepositoryId::new(1),
        edges_vec.clone(),
        underlying_storage.clone(),
    )?;

    let graph = CommitGraph::new(preloaded_storage.clone());
    graph
        .add(&ctx, name_cs_id("J"), [name_cs_id("I")].into())
        .await?;

    // Test that fetching from the preloaded storage is equivalent
    // to fetching directly from the underlying storage.
    test_equivalent_storages(
        &ctx,
        underlying_storage.clone(),
        preloaded_storage,
        ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"]
            .into_iter()
            .map(name_cs_id)
            .collect(),
    )
    .await?;

    let preloaded_storage_with_empty_underlying_storage = PreloadedCommitGraphStorage::from_edges(
        RepositoryId::new(1),
        edges_vec.clone(),
        Arc::new(InMemoryCommitGraphStorage::new(RepositoryId::new(1))),
    )?;

    // Test that fetching any of the preloaded edges doesn't
    // need to be fetched from the underlying storage by using
    // an empty underlying storage.
    test_equivalent_storages(
        &ctx,
        underlying_storage,
        preloaded_storage_with_empty_underlying_storage.clone(),
        ["A", "B", "C", "D", "E"]
            .into_iter()
            .map(name_cs_id)
            .collect(),
    )
    .await?;

    assert_eq!(
        preloaded_storage_with_empty_underlying_storage
            .maybe_fetch_edges(&ctx, name_cs_id("F"))
            .await?,
        None
    );

    Ok(())
}
