/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;
use std::sync::Arc;

use anyhow::Result;
use commit_graph::storage::CommitGraphStorage;
use commit_graph::CommitGraph;
use context::CoreContext;
use in_memory_commit_graph_storage::InMemoryCommitGraphStorage;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::RepositoryId;
use smallvec::smallvec;

use crate::utils::*;

mod utils;

pub async fn test_storage_store_and_fetch(
    ctx: &CoreContext,
    storage: Arc<dyn CommitGraphStorage>,
) -> Result<()> {
    let graph = from_dag(
        ctx,
        r##"
             A-B-C-D-G-H-I
              \     /
               E---F
         "##,
        storage.clone(),
    )
    .await?;

    // Check the public API.
    assert!(graph.exists(ctx, name_cs_id("A")).await?);

    assert!(!graph.exists(ctx, name_cs_id("nonexistent")).await?);
    assert_eq!(
        graph
            .changeset_generation(ctx, name_cs_id("G"))
            .await?
            .unwrap()
            .value(),
        5
    );
    assert_eq!(
        graph
            .changeset_parents(ctx, name_cs_id("A"))
            .await?
            .unwrap()
            .as_slice(),
        &[]
    );
    assert_eq!(
        graph
            .changeset_parents(ctx, name_cs_id("E"))
            .await?
            .unwrap()
            .as_slice(),
        &[name_cs_id("A")]
    );
    assert_eq!(
        graph
            .changeset_parents(ctx, name_cs_id("G"))
            .await?
            .unwrap()
            .as_slice(),
        &[name_cs_id("D"), name_cs_id("F")]
    );

    assert!(
        graph
            .is_ancestor(ctx, name_cs_id("C"), name_cs_id("C"))
            .await?
    );
    assert!(
        graph
            .is_ancestor(ctx, name_cs_id("A"), name_cs_id("H"))
            .await?
    );
    assert!(
        graph
            .is_ancestor(ctx, name_cs_id("A"), name_cs_id("F"))
            .await?
    );
    assert!(
        graph
            .is_ancestor(ctx, name_cs_id("F"), name_cs_id("I"))
            .await?
    );
    assert!(
        graph
            .is_ancestor(ctx, name_cs_id("C"), name_cs_id("I"))
            .await?
    );
    assert!(
        !graph
            .is_ancestor(ctx, name_cs_id("I"), name_cs_id("A"))
            .await?
    );
    assert!(
        !graph
            .is_ancestor(ctx, name_cs_id("E"), name_cs_id("D"))
            .await?
    );
    assert!(
        !graph
            .is_ancestor(ctx, name_cs_id("B"), name_cs_id("E"))
            .await?
    );

    // Check some underlying storage details.
    assert_eq!(
        storage
            .fetch_edges(ctx, name_cs_id("A"))
            .await?
            .unwrap()
            .merge_ancestor,
        None
    );
    assert_eq!(
        storage
            .fetch_edges(ctx, name_cs_id("C"))
            .await?
            .unwrap()
            .merge_ancestor,
        Some(name_cs_node("A", 1, 0, 0))
    );
    assert_eq!(
        storage
            .fetch_edges(ctx, name_cs_id("I"))
            .await?
            .unwrap()
            .merge_ancestor,
        Some(name_cs_node("G", 5, 1, 4))
    );

    Ok(())
}

pub async fn test_skip_tree(ctx: &CoreContext, storage: Arc<dyn CommitGraphStorage>) -> Result<()> {
    let graph = from_dag(
        ctx,
        r##"
         A-B-C-D-G-H---J-K
            \   /   \ /
             E-F     I

         L-M-N-O-P-Q-R-S-T-U
         "##,
        storage.clone(),
    )
    .await?;

    assert_eq!(
        storage
            .fetch_edges(ctx, name_cs_id("K"))
            .await?
            .unwrap()
            .node
            .cs_id,
        name_cs_id("K")
    );

    assert_skip_tree_parent(&storage, ctx, "G", "B").await?;
    assert_skip_tree_parent(&storage, ctx, "K", "J").await?;
    assert_skip_tree_parent(&storage, ctx, "J", "H").await?;
    assert_skip_tree_parent(&storage, ctx, "H", "G").await?;

    assert_skip_tree_skew_ancestor(&storage, ctx, "H", "A").await?;
    assert_skip_tree_skew_ancestor(&storage, ctx, "K", "J").await?;
    assert_skip_tree_skew_ancestor(&storage, ctx, "U", "T").await?;
    assert_skip_tree_skew_ancestor(&storage, ctx, "T", "S").await?;
    assert_skip_tree_skew_ancestor(&storage, ctx, "S", "L").await?;

    assert_skip_tree_level_ancestor(&graph, ctx, "S", 4, Some("P")).await?;
    assert_skip_tree_level_ancestor(&graph, ctx, "U", 7, Some("S")).await?;
    assert_skip_tree_level_ancestor(&graph, ctx, "T", 7, Some("S")).await?;
    assert_skip_tree_level_ancestor(&graph, ctx, "O", 2, Some("N")).await?;
    assert_skip_tree_level_ancestor(&graph, ctx, "N", 3, None).await?;
    assert_skip_tree_level_ancestor(&graph, ctx, "K", 2, Some("G")).await?;

    assert_skip_tree_lowest_common_ancestor(&graph, ctx, "D", "F", Some("B")).await?;
    assert_skip_tree_lowest_common_ancestor(&graph, ctx, "K", "I", Some("H")).await?;
    assert_skip_tree_lowest_common_ancestor(&graph, ctx, "D", "C", Some("C")).await?;
    assert_skip_tree_lowest_common_ancestor(&graph, ctx, "N", "K", None).await?;
    assert_skip_tree_lowest_common_ancestor(&graph, ctx, "A", "I", Some("A")).await?;

    Ok(())
}

pub async fn test_p1_linear_tree(
    ctx: &CoreContext,
    storage: Arc<dyn CommitGraphStorage>,
) -> Result<()> {
    let graph = from_dag(
        ctx,
        r##"
         A-B-C-D-G-H---J-K
            \   /   \ /
             E-F     I

         L-M-N-O-P-Q-R-S-T-U
         "##,
        storage.clone(),
    )
    .await?;

    assert_p1_linear_skew_ancestor(&storage, ctx, "A", None).await?;
    assert_p1_linear_skew_ancestor(&storage, ctx, "B", Some("A")).await?;
    assert_p1_linear_skew_ancestor(&storage, ctx, "C", Some("B")).await?;
    assert_p1_linear_skew_ancestor(&storage, ctx, "D", Some("A")).await?;
    assert_p1_linear_skew_ancestor(&storage, ctx, "E", Some("B")).await?;
    assert_p1_linear_skew_ancestor(&storage, ctx, "F", Some("A")).await?;
    assert_p1_linear_skew_ancestor(&storage, ctx, "G", Some("D")).await?;
    assert_p1_linear_skew_ancestor(&storage, ctx, "H", Some("G")).await?;
    assert_p1_linear_skew_ancestor(&storage, ctx, "I", Some("D")).await?;
    assert_p1_linear_skew_ancestor(&storage, ctx, "J", Some("D")).await?;
    assert_p1_linear_skew_ancestor(&storage, ctx, "K", Some("A")).await?;

    assert_p1_linear_level_ancestor(&graph, ctx, "S", 4, Some("P")).await?;
    assert_p1_linear_level_ancestor(&graph, ctx, "U", 7, Some("S")).await?;
    assert_p1_linear_level_ancestor(&graph, ctx, "T", 7, Some("S")).await?;
    assert_p1_linear_level_ancestor(&graph, ctx, "O", 2, Some("N")).await?;
    assert_p1_linear_level_ancestor(&graph, ctx, "N", 3, None).await?;
    assert_p1_linear_level_ancestor(&graph, ctx, "K", 2, Some("C")).await?;

    assert_p1_linear_lowest_common_ancestor(&graph, ctx, "D", "F", Some("B")).await?;
    assert_p1_linear_lowest_common_ancestor(&graph, ctx, "K", "I", Some("H")).await?;
    assert_p1_linear_lowest_common_ancestor(&graph, ctx, "D", "C", Some("C")).await?;
    assert_p1_linear_lowest_common_ancestor(&graph, ctx, "N", "K", None).await?;
    assert_p1_linear_lowest_common_ancestor(&graph, ctx, "A", "I", Some("A")).await?;

    Ok(())
}

pub async fn test_get_ancestors_difference(
    ctx: &CoreContext,
    storage: Arc<dyn CommitGraphStorage>,
) -> Result<()> {
    let graph = from_dag(
        ctx,
        r##"
         A-B-C-D-G-H---J-K
            \   /   \ /
             E-F     I

         L-M-N-O-P-Q-R-S-T-U
         "##,
        storage.clone(),
    )
    .await?;

    assert_get_ancestors_difference(
        &graph,
        ctx,
        vec!["K"],
        vec![],
        vec!["K", "J", "I", "H", "G", "D", "F", "C", "E", "B", "A"],
    )
    .await?;

    assert_get_ancestors_difference(
        &graph,
        ctx,
        vec!["K", "U"],
        vec![],
        vec![
            "U", "T", "S", "R", "Q", "P", "O", "N", "M", "L", "K", "J", "I", "H", "G", "D", "F",
            "C", "E", "B", "A",
        ],
    )
    .await?;

    assert_get_ancestors_difference(&graph, ctx, vec!["K"], vec!["G"], vec!["K", "J", "I", "H"])
        .await?;

    assert_get_ancestors_difference(&graph, ctx, vec!["K", "I"], vec!["J"], vec!["K"]).await?;

    assert_get_ancestors_difference(
        &graph,
        ctx,
        vec!["I"],
        vec!["C"],
        vec!["I", "H", "G", "F", "E", "D"],
    )
    .await?;

    assert_get_ancestors_difference(
        &graph,
        ctx,
        vec!["J", "S"],
        vec!["C", "E", "O"],
        vec!["J", "I", "H", "G", "F", "D", "S", "R", "Q", "P"],
    )
    .await?;

    Ok(())
}

pub async fn test_find_by_prefix(
    ctx: &CoreContext,
    storage: Arc<dyn CommitGraphStorage>,
) -> Result<()> {
    let graph = from_dag(
        ctx,
        r##"
             J-K-L-LZZ
             M-MA-MAA-MAB-MAC
             M-MB-MBB-MBC
             N-NAA
             O-P-QQ
             a-b-c
         "##,
        storage.clone(),
    )
    .await?;

    assert_eq!(
        graph
            .find_by_prefix(ctx, ChangesetIdPrefix::from_bytes("Z")?, 10)
            .await?,
        ChangesetIdsResolvedFromPrefix::NoMatch
    );
    assert_eq!(
        graph
            .find_by_prefix(ctx, ChangesetIdPrefix::from_bytes("Q")?, 10)
            .await?,
        ChangesetIdsResolvedFromPrefix::Single(name_cs_id("QQ"))
    );
    assert_eq!(
        graph
            .find_by_prefix(ctx, ChangesetIdPrefix::from_bytes("MA")?, 10)
            .await?,
        ChangesetIdsResolvedFromPrefix::Multiple(vec![
            name_cs_id("MA"),
            name_cs_id("MAA"),
            name_cs_id("MAB"),
            name_cs_id("MAC"),
        ])
    );
    assert_eq!(
        graph
            .find_by_prefix(ctx, ChangesetIdPrefix::from_bytes("M")?, 6)
            .await?,
        ChangesetIdsResolvedFromPrefix::TooMany(vec![
            name_cs_id("M"),
            name_cs_id("MA"),
            name_cs_id("MAA"),
            name_cs_id("MAB"),
            name_cs_id("MAC"),
            name_cs_id("MB"),
        ])
    );
    // Check prefixes that are not a full byte. `P` is `\x50` in ASCII.
    assert_eq!(
        graph
            .find_by_prefix(ctx, ChangesetIdPrefix::from_str("5")?, 2)
            .await?,
        ChangesetIdsResolvedFromPrefix::Multiple(vec![name_cs_id("P"), name_cs_id("QQ")])
    );

    Ok(())
}

pub async fn test_add_recursive(
    ctx: &CoreContext,
    storage: Arc<dyn CommitGraphStorage>,
) -> Result<()> {
    let reference_storage = Arc::new(InMemoryCommitGraphStorage::new(RepositoryId::new(1)));

    let reference_graph = Arc::new(
        from_dag(
            ctx,
            r##"
             A-B-C-D-G-H-I
              \     /
               E---F---J
         "##,
            reference_storage,
        )
        .await?,
    );

    let graph = CommitGraph::new(storage);
    assert_eq!(
        graph
            .add_recursive(
                ctx,
                reference_graph.clone(),
                name_cs_id("I"),
                smallvec![name_cs_id("H")],
            )
            .await?,
        9
    );
    assert_eq!(
        graph
            .add_recursive(
                ctx,
                reference_graph,
                name_cs_id("J"),
                smallvec![name_cs_id("F")],
            )
            .await?,
        1
    );

    assert!(graph.exists(ctx, name_cs_id("A")).await?);

    assert_eq!(
        graph
            .changeset_parents(ctx, name_cs_id("E"))
            .await?
            .unwrap()
            .as_slice(),
        &[name_cs_id("A")]
    );
    assert_eq!(
        graph
            .changeset_parents(ctx, name_cs_id("G"))
            .await?
            .unwrap()
            .as_slice(),
        &[name_cs_id("D"), name_cs_id("F")]
    );
    assert_eq!(
        graph
            .changeset_parents(ctx, name_cs_id("I"))
            .await?
            .unwrap()
            .as_slice(),
        &[name_cs_id("H")]
    );
    assert_eq!(
        graph
            .changeset_parents(ctx, name_cs_id("J"))
            .await?
            .unwrap()
            .as_slice(),
        &[name_cs_id("F")]
    );

    Ok(())
}
