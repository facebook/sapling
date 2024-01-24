/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_upcasting)]

use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Result;
use cloned::cloned;
use commit_graph::CommitGraph;
use commit_graph_types::storage::CommitGraphStorage;
use commit_graph_types::storage::Prefetch;
use context::CoreContext;
use in_memory_commit_graph_storage::InMemoryCommitGraphStorage;
use maplit::hashset;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::RepositoryId;
use smallvec::smallvec;
use vec1::vec1;

use crate::utils::*;

#[cfg(test)]
pub mod tests;
pub mod utils;

pub trait CommitGraphStorageTest: CommitGraphStorage {
    fn flush(&self) {}
}

impl CommitGraphStorageTest for InMemoryCommitGraphStorage {}

#[macro_export]
macro_rules! impl_commit_graph_tests {
    ( $test_runner:ident ) => {
        $crate::impl_commit_graph_tests_internal!(
            $test_runner,
            test_storage_store_and_fetch,
            test_skip_tree,
            test_p1_linear_tree,
            test_ancestors_difference,
            test_find_by_prefix,
            test_add_recursive,
            test_add_recursive_many_changesets,
            test_ancestors_frontier_with,
            test_range_stream,
            test_common_base,
            test_slice_ancestors,
            test_children,
            test_ancestors_difference_segments_1,
            test_ancestors_difference_segments_2,
            test_ancestors_difference_segments_3,
            test_locations_to_changeset_ids,
            test_changeset_ids_to_locations,
            test_process_topologically,
            test_minimize_frontier,
        );
    };
}

#[macro_export]
macro_rules! impl_commit_graph_tests_internal {
    ( $test_runner:ident, $($test_name:ident, )* ) => {
        $(
            #[fbinit::test]
            pub async fn $test_name(fb: FacebookInit) -> Result<()> {
                $test_runner(fb, $crate::$test_name).await
            }
        )*
    }
}

pub async fn test_storage_store_and_fetch(
    ctx: CoreContext,
    storage: Arc<dyn CommitGraphStorageTest>,
) -> Result<()> {
    let graph = from_dag(
        &ctx,
        r"
             A-B-C-D-G-H-I
              \     /
               E---F
         ",
        storage.clone(),
    )
    .await?;
    storage.flush();

    // Check the public API.
    assert!(graph.exists(&ctx, name_cs_id("A")).await?);

    assert!(!graph.exists(&ctx, name_cs_id("nonexistent")).await?);
    assert_eq!(
        graph
            .changeset_generation(&ctx, name_cs_id("G"))
            .await?
            .value(),
        5
    );
    assert_eq!(
        graph
            .changeset_parents(&ctx, name_cs_id("A"))
            .await?
            .as_slice(),
        &[]
    );
    assert_eq!(
        graph
            .changeset_parents(&ctx, name_cs_id("E"))
            .await?
            .as_slice(),
        &[name_cs_id("A")]
    );
    assert_eq!(
        graph
            .changeset_parents(&ctx, name_cs_id("G"))
            .await?
            .as_slice(),
        &[name_cs_id("D"), name_cs_id("F")]
    );

    assert!(
        graph
            .is_ancestor(&ctx, name_cs_id("C"), name_cs_id("C"))
            .await?
    );
    assert!(
        graph
            .is_ancestor(&ctx, name_cs_id("A"), name_cs_id("H"))
            .await?
    );
    assert!(
        graph
            .is_ancestor(&ctx, name_cs_id("A"), name_cs_id("F"))
            .await?
    );
    assert!(
        graph
            .is_ancestor(&ctx, name_cs_id("F"), name_cs_id("I"))
            .await?
    );
    assert!(
        graph
            .is_ancestor(&ctx, name_cs_id("C"), name_cs_id("I"))
            .await?
    );
    assert!(
        !graph
            .is_ancestor(&ctx, name_cs_id("I"), name_cs_id("A"))
            .await?
    );
    assert!(
        !graph
            .is_ancestor(&ctx, name_cs_id("E"), name_cs_id("D"))
            .await?
    );
    assert!(
        !graph
            .is_ancestor(&ctx, name_cs_id("B"), name_cs_id("E"))
            .await?
    );

    assert!(
        graph
            .is_ancestor_of_any(
                &ctx,
                name_cs_id("C"),
                vec![name_cs_id("D"), name_cs_id("F")]
            )
            .await?
    );
    assert!(
        graph
            .is_ancestor_of_any(&ctx, name_cs_id("C"), vec![name_cs_id("D")])
            .await?
    );
    assert!(
        graph
            .is_ancestor_of_any(
                &ctx,
                name_cs_id("C"),
                vec![name_cs_id("G"), name_cs_id("I")]
            )
            .await?
    );
    assert!(
        !graph
            .is_ancestor_of_any(
                &ctx,
                name_cs_id("C"),
                vec![name_cs_id("B"), name_cs_id("F")]
            )
            .await?
    );
    assert!(
        graph
            .is_ancestor_of_any(
                &ctx,
                name_cs_id("A"),
                vec![
                    name_cs_id("B"),
                    name_cs_id("C"),
                    name_cs_id("D"),
                    name_cs_id("E"),
                    name_cs_id("F"),
                    name_cs_id("G"),
                    name_cs_id("H"),
                    name_cs_id("I")
                ]
            )
            .await?
    );
    assert!(
        !graph
            .is_ancestor_of_any(
                &ctx,
                name_cs_id("I"),
                vec![name_cs_id("G"), name_cs_id("H")]
            )
            .await?
    );

    // Check some underlying storage details.
    assert_eq!(
        storage
            .maybe_fetch_edges(&ctx, name_cs_id("A"))
            .await?
            .unwrap()
            .merge_ancestor,
        None
    );
    assert_eq!(
        storage
            .maybe_fetch_edges(&ctx, name_cs_id("C"))
            .await?
            .unwrap()
            .merge_ancestor,
        Some(name_cs_node("A", 1, 0, 0))
    );
    assert_eq!(
        storage
            .maybe_fetch_edges(&ctx, name_cs_id("I"))
            .await?
            .unwrap()
            .merge_ancestor,
        Some(name_cs_node("G", 5, 1, 4))
    );

    // fetch_many_edges and maybe_fetch_many_edges return the same result if none of the changesets
    // are missing.
    assert_eq!(
        storage
            .fetch_many_edges(
                &ctx,
                &[name_cs_id("A"), name_cs_id("C"), name_cs_id("I")],
                Prefetch::None
            )
            .await?,
        storage
            .maybe_fetch_many_edges(
                &ctx,
                &[name_cs_id("A"), name_cs_id("C"), name_cs_id("I")],
                Prefetch::None
            )
            .await?,
    );

    // fetch_many_edges returns an error if any of the changesets are missing.
    assert!(
        storage
            .fetch_many_edges(
                &ctx,
                &[name_cs_id("Z"), name_cs_id("A"), name_cs_id("B")],
                Prefetch::None
            )
            .await
            .is_err()
    );

    // maybe_fetch_many_edges ignores missing changesets ("Z" in this case).
    assert_eq!(
        storage
            .maybe_fetch_many_edges(
                &ctx,
                &[
                    name_cs_id("Z"),
                    name_cs_id("A"),
                    name_cs_id("C"),
                    name_cs_id("I")
                ],
                Prefetch::None
            )
            .await?
            .into_keys()
            .collect::<HashSet<_>>(),
        hashset! {name_cs_id("A"), name_cs_id("C"), name_cs_id("I")},
    );

    Ok(())
}

pub async fn test_skip_tree(
    ctx: CoreContext,
    storage: Arc<dyn CommitGraphStorageTest>,
) -> Result<()> {
    let graph = from_dag(
        &ctx,
        r"
         A-B-C-D-G-H---J-K
            \   /   \ /
             E-F     I

         L-M-N-O-P-Q-R-S-T-U
         ",
        storage.clone(),
    )
    .await?;
    storage.flush();

    assert_eq!(
        storage
            .maybe_fetch_edges(&ctx, name_cs_id("K"))
            .await?
            .unwrap()
            .node
            .cs_id,
        name_cs_id("K")
    );

    assert_skip_tree_parent(&storage, &ctx, "G", "B").await?;
    assert_skip_tree_parent(&storage, &ctx, "K", "J").await?;
    assert_skip_tree_parent(&storage, &ctx, "J", "H").await?;
    assert_skip_tree_parent(&storage, &ctx, "H", "G").await?;

    assert_skip_tree_skew_ancestor(&storage, &ctx, "H", "A").await?;
    assert_skip_tree_skew_ancestor(&storage, &ctx, "K", "J").await?;
    assert_skip_tree_skew_ancestor(&storage, &ctx, "U", "T").await?;
    assert_skip_tree_skew_ancestor(&storage, &ctx, "T", "S").await?;
    assert_skip_tree_skew_ancestor(&storage, &ctx, "S", "L").await?;

    assert_skip_tree_level_ancestor(&graph, &ctx, "S", 4, Some("P")).await?;
    assert_skip_tree_level_ancestor(&graph, &ctx, "U", 7, Some("S")).await?;
    assert_skip_tree_level_ancestor(&graph, &ctx, "T", 7, Some("S")).await?;
    assert_skip_tree_level_ancestor(&graph, &ctx, "O", 2, Some("N")).await?;
    assert_skip_tree_level_ancestor(&graph, &ctx, "N", 3, None).await?;
    assert_skip_tree_level_ancestor(&graph, &ctx, "K", 2, Some("G")).await?;

    assert_skip_tree_lowest_common_ancestor(&graph, &ctx, "D", "F", Some("B")).await?;
    assert_skip_tree_lowest_common_ancestor(&graph, &ctx, "K", "I", Some("H")).await?;
    assert_skip_tree_lowest_common_ancestor(&graph, &ctx, "D", "C", Some("C")).await?;
    assert_skip_tree_lowest_common_ancestor(&graph, &ctx, "N", "K", None).await?;
    assert_skip_tree_lowest_common_ancestor(&graph, &ctx, "A", "I", Some("A")).await?;

    Ok(())
}

pub async fn test_p1_linear_tree(
    ctx: CoreContext,
    storage: Arc<dyn CommitGraphStorageTest>,
) -> Result<()> {
    let graph = from_dag(
        &ctx,
        r"
         A-B-C-D-G-H---J-K
            \   /   \ /
             E-F     I

         L-M-N-O-P-Q-R-S-T-U
         ",
        storage.clone(),
    )
    .await?;
    storage.flush();

    assert_p1_linear_skew_ancestor(&storage, &ctx, "A", None).await?;
    assert_p1_linear_skew_ancestor(&storage, &ctx, "B", Some("A")).await?;
    assert_p1_linear_skew_ancestor(&storage, &ctx, "C", Some("B")).await?;
    assert_p1_linear_skew_ancestor(&storage, &ctx, "D", Some("A")).await?;
    assert_p1_linear_skew_ancestor(&storage, &ctx, "E", Some("B")).await?;
    assert_p1_linear_skew_ancestor(&storage, &ctx, "F", Some("A")).await?;
    assert_p1_linear_skew_ancestor(&storage, &ctx, "G", Some("D")).await?;
    assert_p1_linear_skew_ancestor(&storage, &ctx, "H", Some("G")).await?;
    assert_p1_linear_skew_ancestor(&storage, &ctx, "I", Some("D")).await?;
    assert_p1_linear_skew_ancestor(&storage, &ctx, "J", Some("D")).await?;
    assert_p1_linear_skew_ancestor(&storage, &ctx, "K", Some("A")).await?;

    assert_p1_linear_level_ancestor(&graph, &ctx, "S", 4, Some("P")).await?;
    assert_p1_linear_level_ancestor(&graph, &ctx, "U", 7, Some("S")).await?;
    assert_p1_linear_level_ancestor(&graph, &ctx, "T", 7, Some("S")).await?;
    assert_p1_linear_level_ancestor(&graph, &ctx, "O", 2, Some("N")).await?;
    assert_p1_linear_level_ancestor(&graph, &ctx, "N", 3, None).await?;
    assert_p1_linear_level_ancestor(&graph, &ctx, "K", 2, Some("C")).await?;

    assert_p1_linear_lowest_common_ancestor(&graph, &ctx, "D", "F", Some("B")).await?;
    assert_p1_linear_lowest_common_ancestor(&graph, &ctx, "K", "I", Some("H")).await?;
    assert_p1_linear_lowest_common_ancestor(&graph, &ctx, "D", "C", Some("C")).await?;
    assert_p1_linear_lowest_common_ancestor(&graph, &ctx, "N", "K", None).await?;
    assert_p1_linear_lowest_common_ancestor(&graph, &ctx, "A", "I", Some("A")).await?;

    Ok(())
}

pub async fn test_ancestors_difference(
    ctx: CoreContext,
    storage: Arc<dyn CommitGraphStorageTest>,
) -> Result<()> {
    let graph = from_dag(
        &ctx,
        r"
         A-B-C-D-G-H---J-K
            \   /   \ /
             E-F     I

         L-M-N-O-P-Q-R-S-T-U
         ",
        storage.clone(),
    )
    .await?;
    storage.flush();

    assert_ancestors_difference(
        &graph,
        &ctx,
        vec!["K"],
        vec![],
        vec!["K", "J", "I", "H", "G", "D", "F", "C", "E", "B", "A"],
    )
    .await?;

    assert_ancestors_difference(
        &graph,
        &ctx,
        vec!["K", "U"],
        vec![],
        vec![
            "U", "T", "S", "R", "Q", "P", "O", "N", "M", "L", "K", "J", "I", "H", "G", "D", "F",
            "C", "E", "B", "A",
        ],
    )
    .await?;

    assert_ancestors_difference(&graph, &ctx, vec!["K"], vec!["G"], vec!["K", "J", "I", "H"])
        .await?;

    assert_ancestors_difference(&graph, &ctx, vec!["K", "I"], vec!["J"], vec!["K"]).await?;

    assert_ancestors_difference(
        &graph,
        &ctx,
        vec!["I"],
        vec!["C"],
        vec!["I", "H", "G", "F", "E", "D"],
    )
    .await?;

    assert_ancestors_difference(
        &graph,
        &ctx,
        vec!["J", "S"],
        vec!["C", "E", "O"],
        vec!["J", "I", "H", "G", "F", "D", "S", "R", "Q", "P"],
    )
    .await?;

    let set1 = ["A", "B", "C", "D", "E", "F", "G", "H", "I"]
        .into_iter()
        .map(name_cs_id)
        .collect::<HashSet<_>>();

    let set1_fn = move |cs_id| {
        cloned!(set1);
        async move { Ok(set1.contains(&cs_id)) }
    };

    assert_ancestors_difference_with(
        &graph,
        &ctx,
        vec!["J", "S"],
        vec!["C", "E", "O"],
        set1_fn.clone(),
        vec!["J", "S", "R", "Q", "P"],
    )
    .await?;

    assert_ancestors_difference_with(
        &graph,
        &ctx,
        vec!["K"],
        vec!["C", "E"],
        set1_fn.clone(),
        vec!["K", "J"],
    )
    .await?;

    let set2 = ["A", "B", "C"]
        .into_iter()
        .map(name_cs_id)
        .collect::<HashSet<_>>();

    let set2_fn = move |cs_id| {
        cloned!(set2);
        async move { Ok(set2.contains(&cs_id)) }
    };

    assert_ancestors_difference_with(
        &graph,
        &ctx,
        vec!["H"],
        vec![],
        set2_fn.clone(),
        vec!["D", "E", "F", "G", "H"],
    )
    .await?;

    assert_ancestors_difference_with(
        &graph,
        &ctx,
        vec!["H"],
        vec!["F"],
        set2_fn.clone(),
        vec!["D", "G", "H"],
    )
    .await?;

    Ok(())
}

pub async fn test_find_by_prefix(
    ctx: CoreContext,
    storage: Arc<dyn CommitGraphStorageTest>,
) -> Result<()> {
    let graph = from_dag(
        &ctx,
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
    storage.flush();

    assert_eq!(
        graph
            .find_by_prefix(&ctx, ChangesetIdPrefix::from_bytes("Z")?, 10)
            .await?,
        ChangesetIdsResolvedFromPrefix::NoMatch
    );
    assert_eq!(
        graph
            .find_by_prefix(&ctx, ChangesetIdPrefix::from_bytes("Q")?, 10)
            .await?,
        ChangesetIdsResolvedFromPrefix::Single(name_cs_id("QQ"))
    );
    assert_eq!(
        graph
            .find_by_prefix(&ctx, ChangesetIdPrefix::from_bytes("MA")?, 10)
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
            .find_by_prefix(&ctx, ChangesetIdPrefix::from_bytes("M")?, 6)
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
            .find_by_prefix(&ctx, ChangesetIdPrefix::from_str("5")?, 2)
            .await?,
        ChangesetIdsResolvedFromPrefix::Multiple(vec![name_cs_id("P"), name_cs_id("QQ")])
    );

    Ok(())
}

pub async fn test_add_recursive(
    ctx: CoreContext,
    storage: Arc<dyn CommitGraphStorageTest>,
) -> Result<()> {
    let reference_storage = Arc::new(InMemoryCommitGraphStorage::new(RepositoryId::new(1)));

    let reference_graph = Arc::new(
        from_dag(
            &ctx,
            r"
             A-B-C-D-G-H-I
              \     /
               E---F---J
         ",
            reference_storage,
        )
        .await?,
    );

    let graph = CommitGraph::new(storage.clone());
    assert_eq!(
        graph
            .add_recursive(
                &ctx,
                reference_graph.clone(),
                vec1![(name_cs_id("I"), smallvec![name_cs_id("H")])],
            )
            .await?,
        9
    );
    assert_eq!(
        graph
            .add_recursive(
                &ctx,
                reference_graph,
                vec1![(name_cs_id("J"), smallvec![name_cs_id("F")])]
            )
            .await?,
        1
    );
    storage.flush();

    assert!(graph.exists(&ctx, name_cs_id("A")).await?);

    assert_eq!(
        graph
            .changeset_parents(&ctx, name_cs_id("E"))
            .await?
            .as_slice(),
        &[name_cs_id("A")]
    );
    assert_eq!(
        graph
            .changeset_parents(&ctx, name_cs_id("G"))
            .await?
            .as_slice(),
        &[name_cs_id("D"), name_cs_id("F")]
    );
    assert_eq!(
        graph
            .changeset_parents(&ctx, name_cs_id("I"))
            .await?
            .as_slice(),
        &[name_cs_id("H")]
    );
    assert_eq!(
        graph
            .changeset_parents(&ctx, name_cs_id("J"))
            .await?
            .as_slice(),
        &[name_cs_id("F")]
    );

    Ok(())
}

pub async fn test_add_recursive_many_changesets(
    ctx: CoreContext,
    storage: Arc<dyn CommitGraphStorageTest>,
) -> Result<()> {
    let reference_storage = Arc::new(InMemoryCommitGraphStorage::new(RepositoryId::new(1)));

    let reference_graph = Arc::new(
        from_dag(
            &ctx,
            r"
             A-B-C-D-G-H-I
              \     /
               E---F---J
         ",
            reference_storage,
        )
        .await?,
    );

    let graph = CommitGraph::new(storage.clone());
    assert_eq!(
        graph
            .add_recursive(
                &ctx,
                reference_graph.clone(),
                vec1![
                    (name_cs_id("I"), smallvec![name_cs_id("H")]),
                    (name_cs_id("K"), smallvec![name_cs_id("I")]),
                    (name_cs_id("L"), smallvec![name_cs_id("K")]),
                    (name_cs_id("M"), smallvec![name_cs_id("J")]),
                ],
            )
            .await?,
        13
    );
    storage.flush();

    assert_eq!(
        graph
            .changeset_parents(&ctx, name_cs_id("I"))
            .await?
            .as_slice(),
        &[name_cs_id("H")]
    );
    assert_eq!(
        graph
            .changeset_parents(&ctx, name_cs_id("K"))
            .await?
            .as_slice(),
        &[name_cs_id("I")]
    );
    assert_eq!(
        graph
            .changeset_parents(&ctx, name_cs_id("L"))
            .await?
            .as_slice(),
        &[name_cs_id("K")]
    );

    assert_eq!(
        graph
            .add_recursive(
                &ctx,
                reference_graph.clone(),
                vec1![
                    (name_cs_id("N"), smallvec![name_cs_id("M")]),
                    (name_cs_id("O"), smallvec![name_cs_id("K"), name_cs_id("N")]),
                ],
            )
            .await?,
        2
    );
    Ok(())
}

pub async fn test_ancestors_frontier_with(
    ctx: CoreContext,
    storage: Arc<dyn CommitGraphStorageTest>,
) -> Result<()> {
    let graph = from_dag(
        &ctx,
        r"
         A-B-C-D-G-H---J-K
            \   /   \ /
             E-F     I

         L-M-N-O-P-Q-R-S-T-U
         ",
        storage.clone(),
    )
    .await?;
    storage.flush();

    let set1 = ["A", "B", "C", "D", "E", "F", "G", "H", "I"]
        .into_iter()
        .map(name_cs_id)
        .collect::<HashSet<_>>();

    assert_ancestors_frontier_with(
        &graph,
        &ctx,
        vec!["K", "U"],
        move |cs_id| {
            cloned!(set1);
            async move { Ok(set1.contains(&cs_id)) }
        },
        vec!["H", "I"],
    )
    .await?;

    let set2 = ["A", "B", "C", "E"]
        .into_iter()
        .map(name_cs_id)
        .collect::<HashSet<_>>();

    assert_ancestors_frontier_with(
        &graph,
        &ctx,
        vec!["D", "F"],
        {
            cloned!(set2);
            move |cs_id| {
                cloned!(set2);
                async move { Ok(set2.contains(&cs_id)) }
            }
        },
        vec!["C", "E"],
    )
    .await?;

    assert_ancestors_frontier_with(
        &graph,
        &ctx,
        vec!["G"],
        {
            cloned!(set2);
            move |cs_id| {
                cloned!(set2);
                async move { Ok(set2.contains(&cs_id)) }
            }
        },
        vec!["C", "E"],
    )
    .await?;

    assert_ancestors_frontier_with(
        &graph,
        &ctx,
        vec!["K"],
        {
            cloned!(set2);
            move |cs_id| {
                cloned!(set2);
                async move { Ok(set2.contains(&cs_id)) }
            }
        },
        vec!["C", "E"],
    )
    .await?;

    assert_ancestors_frontier_with(
        &graph,
        &ctx,
        vec!["D"],
        {
            cloned!(set2);
            move |cs_id| {
                cloned!(set2);
                async move { Ok(set2.contains(&cs_id)) }
            }
        },
        vec!["C"],
    )
    .await?;

    Ok(())
}

pub async fn test_range_stream(
    ctx: CoreContext,
    storage: Arc<dyn CommitGraphStorageTest>,
) -> Result<()> {
    let graph = from_dag(
        &ctx,
        r"
         A-B-C-D-G-H---J-K
            \   /   \ /
             E-F     I

         L-M-N-O-P-Q-R-S-T-U
         ",
        storage.clone(),
    )
    .await?;
    storage.flush();

    assert_range_stream(
        &graph,
        &ctx,
        "A",
        "K",
        vec!["A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K"],
    )
    .await?;
    assert_range_stream(&graph, &ctx, "D", "K", vec!["D", "G", "H", "I", "J", "K"]).await?;
    assert_range_stream(&graph, &ctx, "A", "U", vec![]).await?;
    assert_range_stream(&graph, &ctx, "O", "T", vec!["O", "P", "Q", "R", "S", "T"]).await?;

    Ok(())
}

pub async fn test_common_base(
    ctx: CoreContext,
    storage: Arc<dyn CommitGraphStorageTest>,
) -> Result<()> {
    let graph = from_dag(
        &ctx,
        r"
        A-B-C-D-E-L------N
           \       \    /
            F-G-H   M  /
             \     /  /
              I-J-K--/

        O-P-Q-R-S-T-U-V-W
        ",
        storage.clone(),
    )
    .await?;
    storage.flush();

    assert_common_base(&graph, &ctx, "J", "J", vec!["J"]).await?;
    assert_common_base(&graph, &ctx, "K", "J", vec!["J"]).await?;
    assert_common_base(&graph, &ctx, "E", "H", vec!["B"]).await?;
    assert_common_base(&graph, &ctx, "G", "J", vec!["F"]).await?;
    assert_common_base(&graph, &ctx, "M", "N", vec!["K", "L"]).await?;
    assert_common_base(&graph, &ctx, "L", "K", vec!["B"]).await?;
    assert_common_base(&graph, &ctx, "M", "H", vec!["F"]).await?;
    assert_common_base(&graph, &ctx, "A", "B", vec!["A"]).await?;
    assert_common_base(&graph, &ctx, "N", "W", vec![]).await?;
    assert_common_base(&graph, &ctx, "D", "Q", vec![]).await?;

    Ok(())
}

pub async fn test_slice_ancestors(
    ctx: CoreContext,
    storage: Arc<dyn CommitGraphStorageTest>,
) -> Result<()> {
    let graph = from_dag(
        &ctx,
        r"
         A-B-C-D-G-H---J-K
            \   /   \ /
             E-F     I

         L-M-N-O-P-Q-R-S-T-U
         ",
        storage.clone(),
    )
    .await?;
    storage.flush();

    assert_slice_ancestors(
        &graph,
        &ctx,
        vec!["H"],
        |cs_ids| async { Ok(cs_ids.into_iter().collect::<HashSet<_>>()) },
        2,
        vec![(1, vec!["B"]), (3, vec!["D", "F"]), (5, vec!["H"])],
    )
    .await?;

    assert_slice_ancestors(
        &graph,
        &ctx,
        vec!["Q"],
        |cs_ids| async { Ok(cs_ids.into_iter().collect::<HashSet<_>>()) },
        1,
        vec![
            (1, vec!["L"]),
            (2, vec!["M"]),
            (3, vec!["N"]),
            (4, vec!["O"]),
            (5, vec!["P"]),
            (6, vec!["Q"]),
        ],
    )
    .await?;

    assert_slice_ancestors(
        &graph,
        &ctx,
        vec!["Q"],
        |cs_ids| async { Ok(cs_ids.into_iter().collect::<HashSet<_>>()) },
        3,
        vec![(1, vec!["N"]), (4, vec!["Q"])],
    )
    .await?;

    let set1 = ["P", "Q", "R", "S", "T", "U"]
        .into_iter()
        .map(name_cs_id)
        .collect::<HashSet<_>>();

    assert_slice_ancestors(
        &graph,
        &ctx,
        vec!["Q"],
        |_| async { Ok(set1.clone()) },
        1,
        vec![(5, vec!["P"]), (6, vec!["Q"])],
    )
    .await?;

    Ok(())
}

pub async fn test_children(
    ctx: CoreContext,
    storage: Arc<dyn CommitGraphStorageTest>,
) -> Result<()> {
    let graph = from_dag(
        &ctx,
        r"
        A-B-C-D-E-L------N
           \       \    /
            F-G-H   M  /
             \     /  /
              I-J-K--/
        ",
        storage.clone(),
    )
    .await?;
    storage.flush();

    assert_children(&graph, &ctx, "A", vec!["B"]).await?;
    assert_children(&graph, &ctx, "B", vec!["C", "F"]).await?;
    assert_children(&graph, &ctx, "C", vec!["D"]).await?;
    assert_children(&graph, &ctx, "D", vec!["E"]).await?;
    assert_children(&graph, &ctx, "E", vec!["L"]).await?;
    assert_children(&graph, &ctx, "F", vec!["G", "I"]).await?;
    assert_children(&graph, &ctx, "G", vec!["H"]).await?;
    assert_children(&graph, &ctx, "H", vec![]).await?;
    assert_children(&graph, &ctx, "I", vec!["J"]).await?;
    assert_children(&graph, &ctx, "J", vec!["K"]).await?;
    assert_children(&graph, &ctx, "K", vec!["M", "N"]).await?;
    assert_children(&graph, &ctx, "L", vec!["M", "N"]).await?;
    assert_children(&graph, &ctx, "M", vec![]).await?;
    assert_children(&graph, &ctx, "N", vec![]).await?;

    Ok(())
}

pub async fn test_ancestors_difference_segments_1(
    ctx: CoreContext,
    storage: Arc<dyn CommitGraphStorageTest>,
) -> Result<()> {
    let graph = from_dag(
        &ctx,
        r"
        A-B-C-D-E---L------N----O
           \         \    /
            F-G-H     M  /
             \       /  /
              I-J---K--/---Q---R
                 \
                  \---------P
        ",
        storage.clone(),
    )
    .await?;
    storage.flush();

    assert_ancestors_difference_segments(&ctx, &graph, vec!["N"], vec![], 3).await?;
    assert_ancestors_difference_segments(&ctx, &graph, vec!["N"], vec!["D"], 3).await?;
    assert_ancestors_difference_segments(&ctx, &graph, vec!["H"], vec!["G"], 1).await?;
    assert_ancestors_difference_segments(&ctx, &graph, vec!["M"], vec![], 3).await?;
    assert_ancestors_difference_segments(&ctx, &graph, vec!["M"], vec!["H"], 3).await?;
    assert_ancestors_difference_segments(&ctx, &graph, vec!["N"], vec!["E", "J"], 3).await?;
    assert_ancestors_difference_segments(&ctx, &graph, vec!["O", "P"], vec![], 4).await?;
    assert_ancestors_difference_segments(&ctx, &graph, vec!["O", "P"], vec!["H"], 4).await?;
    assert_ancestors_difference_segments(&ctx, &graph, vec!["O", "P"], vec!["D", "I"], 4).await?;
    assert_ancestors_difference_segments(&ctx, &graph, vec!["F"], vec!["H"], 0).await?;
    assert_ancestors_difference_segments(&ctx, &graph, vec!["M"], vec!["K"], 2).await?;
    assert_ancestors_difference_segments(&ctx, &graph, vec!["N", "R"], vec![], 3).await?;

    Ok(())
}

pub async fn test_ancestors_difference_segments_2(
    ctx: CoreContext,
    storage: Arc<dyn CommitGraphStorageTest>,
) -> Result<()> {
    let graph = from_dag(
        &ctx,
        r"
        A--B------C----E---J---K
         \  \      \
          \  \--D   \-----F----L
           \  \            \
            \  \--G---H     \--M
             \     \
              \-P   \--I--N----O
        ",
        storage.clone(),
    )
    .await?;
    storage.flush();

    assert_ancestors_difference_segments(&ctx, &graph, vec!["K"], vec![], 1).await?;
    assert_ancestors_difference_segments(&ctx, &graph, vec!["L"], vec![], 1).await?;
    assert_ancestors_difference_segments(&ctx, &graph, vec!["M"], vec![], 1).await?;
    assert_ancestors_difference_segments(&ctx, &graph, vec!["O"], vec![], 1).await?;
    assert_ancestors_difference_segments(&ctx, &graph, vec!["K", "L"], vec![], 2).await?;
    assert_ancestors_difference_segments(&ctx, &graph, vec!["K", "L", "M", "O"], vec![], 4).await?;
    assert_ancestors_difference_segments(&ctx, &graph, vec!["K", "L"], vec!["M"], 2).await?;
    assert_ancestors_difference_segments(&ctx, &graph, vec!["K", "L", "H"], vec!["M", "O"], 3)
        .await?;
    assert_ancestors_difference_segments(&ctx, &graph, vec!["C"], vec!["M"], 0).await?;
    assert_ancestors_difference_segments(&ctx, &graph, vec!["A", "B", "E"], vec![], 1).await?;
    assert_ancestors_difference_segments(&ctx, &graph, vec!["B", "H", "O"], vec!["D"], 2).await?;
    assert_ancestors_difference_segments(&ctx, &graph, vec!["E", "L", "K"], vec!["J"], 2).await?;
    assert_ancestors_difference_segments(
        &ctx,
        &graph,
        vec![
            "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P",
        ],
        vec![],
        7,
    )
    .await?;
    Ok(())
}

pub async fn test_ancestors_difference_segments_3(
    ctx: CoreContext,
    storage: Arc<dyn CommitGraphStorageTest>,
) -> Result<()> {
    let graph = from_dag(
        &ctx,
        r"
        A--B--C--D
            \  \
             E--F
        ",
        storage.clone(),
    )
    .await?;
    storage.flush();

    assert_ancestors_difference_segments(&ctx, &graph, vec!["F"], vec!["D"], 2).await?;

    Ok(())
}

pub async fn test_locations_to_changeset_ids(
    ctx: CoreContext,
    storage: Arc<dyn CommitGraphStorageTest>,
) -> Result<()> {
    let graph = from_dag(
        &ctx,
        r"
        A-B-C-D-E---L------N----O
           \         \    /
            F-G-H     M  /
             \       /  /
              I-J---K--/---Q---R
                 \
                  \---------P
        ",
        storage.clone(),
    )
    .await?;
    storage.flush();

    assert_locations_to_changeset_ids(&ctx, &graph, "L", 2, 4, vec!["D", "C", "B", "A"]).await?;
    assert_locations_to_changeset_ids(&ctx, &graph, "H", 0, 5, vec!["H", "G", "F", "B", "A"])
        .await?;
    assert_locations_to_changeset_ids(&ctx, &graph, "R", 1, 2, vec!["Q", "K"]).await?;
    assert_locations_to_changeset_ids(&ctx, &graph, "R", 2, 2, vec!["K", "J"]).await?;
    assert_locations_to_changeset_ids(&ctx, &graph, "R", 3, 2, vec!["J", "I"]).await?;
    assert_locations_to_changeset_ids(&ctx, &graph, "R", 4, 2, vec!["I", "F"]).await?;
    assert_locations_to_changeset_ids(&ctx, &graph, "M", 0, 1, vec!["M"]).await?;
    assert_locations_to_changeset_ids_errors(&ctx, &graph, "M", 1, 1).await?;
    assert_locations_to_changeset_ids(&ctx, &graph, "O", 0, 1, vec!["O"]).await?;
    assert_locations_to_changeset_ids(&ctx, &graph, "O", 0, 2, vec!["O", "N"]).await?;
    assert_locations_to_changeset_ids_errors(&ctx, &graph, "O", 0, 3).await?;

    Ok(())
}

pub async fn test_changeset_ids_to_locations(
    ctx: CoreContext,
    storage: Arc<dyn CommitGraphStorageTest>,
) -> Result<()> {
    let graph = from_dag(
        &ctx,
        r"
        A-B-C-D-E---L------N----O
           \         \    /
            F-G-H     M  /
             \       /  /
              I-J---K--/---Q---R
                 \
                  \---------P
        ",
        storage.clone(),
    )
    .await?;
    storage.flush();

    assert_changeset_ids_to_locations(
        &ctx,
        &graph,
        vec!["O"],
        vec![
            "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q",
            "R",
        ],
    )
    .await?;
    assert_changeset_ids_to_locations(
        &ctx,
        &graph,
        vec!["O", "R"],
        vec![
            "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q",
            "R",
        ],
    )
    .await?;
    assert_changeset_ids_to_locations(
        &ctx,
        &graph,
        vec!["O", "R", "P"],
        vec![
            "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q",
            "R",
        ],
    )
    .await?;

    Ok(())
}

pub async fn test_process_topologically(
    ctx: CoreContext,
    storage: Arc<dyn CommitGraphStorageTest>,
) -> Result<()> {
    let graph = from_dag(
        &ctx,
        r"
        A--B--C--D--E--F
         \
          G--H---I--J--K
           \    /
            L--M
        ",
        storage.clone(),
    )
    .await?;
    storage.flush();

    assert_process_topologically(
        &ctx,
        &graph,
        vec![
            "I", "J", "K", "F", "B", "C", "G", "H", "L", "D", "E", "M", "A",
        ],
    )
    .await?;
    assert_process_topologically(&ctx, &graph, vec!["F", "C", "A", "B", "E", "D"]).await?;
    assert_process_topologically(&ctx, &graph, vec!["H", "C", "L"]).await?;
    assert_process_topologically(&ctx, &graph, vec!["B", "C", "J", "I"]).await?;
    assert_process_topologically(&ctx, &graph, vec![]).await?;

    Ok(())
}

pub async fn test_minimize_frontier(
    ctx: CoreContext,
    storage: Arc<dyn CommitGraphStorageTest>,
) -> Result<()> {
    let graph = from_dag(
        &ctx,
        r"
        A-B-C-D-E-L------N
           \       \    /
            F-G-H   M  /
             \     /  /
              I-J-K--/
        ",
        storage.clone(),
    )
    .await?;
    storage.flush();

    assert_minimize_frontier(&ctx, &graph, vec!["L", "M", "N"], vec!["M", "N"]).await?;
    assert_minimize_frontier(&ctx, &graph, vec!["A", "B", "C", "D"], vec!["D"]).await?;
    assert_minimize_frontier(&ctx, &graph, vec!["D", "L", "I", "K"], vec!["L", "K"]).await?;
    assert_minimize_frontier(&ctx, &graph, vec![], vec![]).await?;
    assert_minimize_frontier(&ctx, &graph, vec!["B", "C", "H"], vec!["C", "H"]).await?;
    assert_minimize_frontier(
        &ctx,
        &graph,
        vec![
            "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N",
        ],
        vec!["H", "M", "N"],
    )
    .await?;

    Ok(())
}
