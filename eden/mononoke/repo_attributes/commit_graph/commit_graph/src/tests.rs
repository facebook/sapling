/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Result;
use context::CoreContext;
use fbinit::FacebookInit;
use mononoke_types::ChangesetId;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::Generation;
use mononoke_types::RepositoryId;

use crate::edges::ChangesetNode;
use crate::storage::InMemoryCommitGraphStorage;
use crate::CommitGraph;

/// Generate a fake changeset id for graph testing purposes by using the raw
/// bytes of the changeset name, padded with zeroes.
fn name_cs_id(name: &str) -> ChangesetId {
    let mut bytes = [0; 32];
    bytes[..name.len()].copy_from_slice(name.as_bytes());
    ChangesetId::from_bytes(bytes).expect("Changeset ID should be valid")
}

/// Generate a fake changeset node for graph testing purposes by using the raw
/// bytes of the changeset name, padded with zeroes.
fn name_cs_node(name: &str, gen: u64, skip_tree_depth: u64, p1_linear_depth: u64) -> ChangesetNode {
    let cs_id = name_cs_id(name);
    let generation = Generation::new(gen);
    ChangesetNode {
        cs_id,
        generation,
        skip_tree_depth,
        p1_linear_depth,
    }
}

async fn assert_skip_tree_parent(
    graph: &CommitGraph,
    ctx: &CoreContext,
    u: &str,
    u_skip_tree_parent: &str,
) -> Result<()> {
    assert_eq!(
        graph
            .storage
            .fetch_edges(ctx, name_cs_id(u))
            .await?
            .unwrap()
            .skip_tree_parent
            .map(|node| node.cs_id),
        Some(name_cs_id(u_skip_tree_parent))
    );
    Ok(())
}

async fn assert_skip_tree_skew_ancestor(
    graph: &CommitGraph,
    ctx: &CoreContext,
    u: &str,
    u_skip_tree_skew_ancestor: &str,
) -> Result<()> {
    assert_eq!(
        graph
            .storage
            .fetch_edges(ctx, name_cs_id(u))
            .await?
            .unwrap()
            .skip_tree_skew_ancestor
            .map(|node| node.cs_id),
        Some(name_cs_id(u_skip_tree_skew_ancestor))
    );
    Ok(())
}

async fn assert_skip_tree_level_ancestor(
    graph: &CommitGraph,
    ctx: &CoreContext,
    u: &str,
    target_depth: u64,
    u_level_ancestor: Option<&str>,
) -> Result<()> {
    assert_eq!(
        graph
            .skip_tree_level_ancestor(ctx, name_cs_id(u), target_depth)
            .await?
            .map(|node| node.cs_id),
        u_level_ancestor.map(name_cs_id)
    );
    Ok(())
}

async fn assert_skip_tree_lowest_common_ancestor(
    graph: &CommitGraph,
    ctx: &CoreContext,
    u: &str,
    v: &str,
    lca: Option<&str>,
) -> Result<()> {
    assert_eq!(
        graph
            .skip_tree_lowest_common_ancestor(ctx, name_cs_id(u), name_cs_id(v))
            .await?
            .map(|node| node.cs_id),
        lca.map(name_cs_id)
    );
    Ok(())
}

/// Build a commit graph from an ASCII-art dag.
async fn from_dag(ctx: &CoreContext, dag: &str) -> Result<CommitGraph> {
    let mut added: BTreeMap<String, ChangesetId> = BTreeMap::new();
    let dag = drawdag::parse(dag);
    let storage = Arc::new(InMemoryCommitGraphStorage::new(RepositoryId::new(1)));
    let graph = CommitGraph::new(storage.clone());

    while added.len() < dag.len() {
        let mut made_progress = false;
        for (name, parents) in dag.iter() {
            if added.contains_key(name) {
                // This node was already added.
                continue;
            }

            if parents.iter().any(|parent| !added.contains_key(parent)) {
                // This node still has unadded parents.
                continue;
            }

            let parent_ids = parents.iter().map(|parent| added[parent].clone()).collect();

            let cs_id = name_cs_id(name);
            graph.add(ctx, cs_id, parent_ids).await?;
            added.insert(name.clone(), cs_id);
            made_progress = true;
        }
        if !made_progress {
            anyhow::bail!("Graph contains cycles");
        }
    }
    Ok(graph)
}

#[fbinit::test]
async fn test_storage_store_and_fetch(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let graph = from_dag(
        &ctx,
        r##"
            A-B-C-D-G-H-I
             \     /
              E---F
        "##,
    )
    .await?;

    // Check the public API.
    assert!(graph.exists(&ctx, name_cs_id("A")).await?);
    assert!(!graph.exists(&ctx, name_cs_id("nonexistent")).await?);
    assert_eq!(
        graph
            .changeset_generation(&ctx, name_cs_id("G"))
            .await?
            .unwrap()
            .value(),
        5
    );
    assert_eq!(
        graph
            .changeset_parents(&ctx, name_cs_id("A"))
            .await?
            .unwrap()
            .as_slice(),
        &[]
    );
    assert_eq!(
        graph
            .changeset_parents(&ctx, name_cs_id("E"))
            .await?
            .unwrap()
            .as_slice(),
        &[name_cs_id("A")]
    );
    assert_eq!(
        graph
            .changeset_parents(&ctx, name_cs_id("G"))
            .await?
            .unwrap()
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

    // Check some underlying storage details.
    assert_eq!(
        graph
            .storage
            .fetch_edges(&ctx, name_cs_id("A"))
            .await?
            .unwrap()
            .merge_ancestor,
        None
    );
    assert_eq!(
        graph
            .storage
            .fetch_edges(&ctx, name_cs_id("C"))
            .await?
            .unwrap()
            .merge_ancestor,
        Some(name_cs_node("A", 1, 0, 0))
    );
    assert_eq!(
        graph
            .storage
            .fetch_edges(&ctx, name_cs_id("I"))
            .await?
            .unwrap()
            .merge_ancestor,
        Some(name_cs_node("G", 5, 1, 4))
    );

    Ok(())
}

#[fbinit::test]
async fn test_skip_tree(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let graph = from_dag(
        &ctx,
        r##"
        A-B-C-D-G-H---J-K
           \   /   \ /
            E-F     I

        L-M-N-O-P-Q-R-S-T-U
        "##,
    )
    .await?;

    assert_eq!(
        graph
            .storage
            .fetch_edges(&ctx, name_cs_id("K"))
            .await?
            .unwrap()
            .node
            .cs_id,
        name_cs_id("K")
    );

    assert_skip_tree_parent(&graph, &ctx, "G", "B").await?;
    assert_skip_tree_parent(&graph, &ctx, "K", "J").await?;
    assert_skip_tree_parent(&graph, &ctx, "J", "H").await?;
    assert_skip_tree_parent(&graph, &ctx, "H", "G").await?;

    assert_skip_tree_skew_ancestor(&graph, &ctx, "H", "A").await?;
    assert_skip_tree_skew_ancestor(&graph, &ctx, "K", "J").await?;
    assert_skip_tree_skew_ancestor(&graph, &ctx, "U", "T").await?;
    assert_skip_tree_skew_ancestor(&graph, &ctx, "T", "S").await?;
    assert_skip_tree_skew_ancestor(&graph, &ctx, "S", "L").await?;

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

#[fbinit::test]
async fn test_find_by_prefix(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
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
    )
    .await?;

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
