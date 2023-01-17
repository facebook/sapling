/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use commit_graph::edges::ChangesetNode;
use commit_graph::storage::CommitGraphStorage;
use commit_graph::CommitGraph;
use context::CoreContext;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;

/// Generate a fake changeset id for graph testing purposes by using the raw
/// bytes of the changeset name, padded with zeroes.
pub fn name_cs_id(name: &str) -> ChangesetId {
    let mut bytes = [0; 32];
    bytes[..name.len()].copy_from_slice(name.as_bytes());
    ChangesetId::from_bytes(bytes).expect("Changeset ID should be valid")
}

/// Generate a fake changeset node for graph testing purposes by using the raw
/// bytes of the changeset name, padded with zeroes.
pub fn name_cs_node(
    name: &str,
    gen: u64,
    skip_tree_depth: u64,
    p1_linear_depth: u64,
) -> ChangesetNode {
    let cs_id = name_cs_id(name);
    let generation = Generation::new(gen);
    ChangesetNode {
        cs_id,
        generation,
        skip_tree_depth,
        p1_linear_depth,
    }
}

/// Build a commit graph from an ASCII-art dag.
pub async fn from_dag(
    ctx: &CoreContext,
    dag: &str,
    storage: Arc<dyn CommitGraphStorage>,
) -> Result<CommitGraph> {
    let mut added: BTreeMap<String, ChangesetId> = BTreeMap::new();
    let dag = drawdag::parse(dag);
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

pub async fn assert_skip_tree_parent(
    storage: &Arc<dyn CommitGraphStorage>,
    ctx: &CoreContext,
    u: &str,
    u_skip_tree_parent: &str,
) -> Result<()> {
    assert_eq!(
        storage
            .fetch_edges(ctx, name_cs_id(u))
            .await?
            .unwrap()
            .skip_tree_parent
            .map(|node| node.cs_id),
        Some(name_cs_id(u_skip_tree_parent))
    );
    Ok(())
}

pub async fn assert_skip_tree_skew_ancestor(
    storage: &Arc<dyn CommitGraphStorage>,
    ctx: &CoreContext,
    u: &str,
    u_skip_tree_skew_ancestor: &str,
) -> Result<()> {
    assert_eq!(
        storage
            .fetch_edges(ctx, name_cs_id(u))
            .await?
            .unwrap()
            .skip_tree_skew_ancestor
            .map(|node| node.cs_id),
        Some(name_cs_id(u_skip_tree_skew_ancestor))
    );
    Ok(())
}

pub async fn assert_skip_tree_level_ancestor(
    graph: &CommitGraph,
    ctx: &CoreContext,
    u: &str,
    target_depth: u64,
    u_level_ancestor: Option<&str>,
) -> Result<()> {
    assert_eq!(
        graph
            .level_ancestor(
                ctx,
                name_cs_id(u),
                target_depth,
                |edges| edges.skip_tree_parent,
                |edges| edges.skip_tree_skew_ancestor,
                |node| node.skip_tree_depth
            )
            .await?
            .map(|node| node.cs_id),
        u_level_ancestor.map(name_cs_id)
    );
    Ok(())
}

pub async fn assert_skip_tree_lowest_common_ancestor(
    graph: &CommitGraph,
    ctx: &CoreContext,
    u: &str,
    v: &str,
    lca: Option<&str>,
) -> Result<()> {
    assert_eq!(
        graph
            .lowest_common_ancestor(
                ctx,
                name_cs_id(u),
                name_cs_id(v),
                |edges| edges.skip_tree_parent,
                |edges| edges.skip_tree_skew_ancestor,
                |node| node.skip_tree_depth
            )
            .await?
            .map(|node| node.cs_id),
        lca.map(name_cs_id)
    );
    Ok(())
}

pub async fn assert_get_ancestors_difference_with(
    graph: &CommitGraph,
    ctx: &CoreContext,
    heads: Vec<&str>,
    common: Vec<&str>,
    property_fn: impl Fn(ChangesetId) -> bool,
    ancestors_difference: Vec<&str>,
) -> Result<()> {
    let heads = heads.into_iter().map(name_cs_id).collect();
    let common = common.into_iter().map(name_cs_id).collect();

    assert_eq!(
        graph
            .get_ancestors_difference_with(ctx, heads, common, property_fn)
            .await?
            .into_iter()
            .collect::<HashSet<_>>(),
        ancestors_difference
            .into_iter()
            .map(name_cs_id)
            .collect::<HashSet<_>>()
    );
    Ok(())
}

pub async fn assert_get_ancestors_difference(
    graph: &CommitGraph,
    ctx: &CoreContext,
    heads: Vec<&str>,
    common: Vec<&str>,
    ancestors_difference: Vec<&str>,
) -> Result<()> {
    let heads = heads.into_iter().map(name_cs_id).collect();
    let common = common.into_iter().map(name_cs_id).collect();

    assert_eq!(
        graph
            .get_ancestors_difference(ctx, heads, common)
            .await?
            .into_iter()
            .collect::<HashSet<_>>(),
        ancestors_difference
            .into_iter()
            .map(name_cs_id)
            .collect::<HashSet<_>>()
    );
    Ok(())
}

pub async fn assert_get_ancestors_frontier_with(
    graph: &CommitGraph,
    ctx: &CoreContext,
    heads: Vec<&str>,
    property_fn: impl Fn(ChangesetId) -> bool,
    ancestors_frontier: Vec<&str>,
) -> Result<()> {
    let heads = heads.into_iter().map(name_cs_id).collect();

    assert_eq!(
        graph
            .get_ancestors_frontier_with(ctx, heads, property_fn)
            .await?
            .into_iter()
            .collect::<HashSet<_>>(),
        ancestors_frontier
            .into_iter()
            .map(name_cs_id)
            .collect::<HashSet<_>>()
    );
    Ok(())
}

pub async fn assert_p1_linear_skew_ancestor(
    storage: &Arc<dyn CommitGraphStorage>,
    ctx: &CoreContext,
    u: &str,
    u_p1_linear_skew_ancestor: Option<&str>,
) -> Result<()> {
    assert_eq!(
        storage
            .fetch_edges(ctx, name_cs_id(u))
            .await?
            .unwrap()
            .p1_linear_skew_ancestor
            .map(|node| node.cs_id),
        u_p1_linear_skew_ancestor.map(name_cs_id)
    );
    Ok(())
}

pub async fn assert_p1_linear_level_ancestor(
    graph: &CommitGraph,
    ctx: &CoreContext,
    u: &str,
    target_depth: u64,
    u_level_ancestor: Option<&str>,
) -> Result<()> {
    assert_eq!(
        graph
            .level_ancestor(
                ctx,
                name_cs_id(u),
                target_depth,
                |edges| edges.parents.first().copied(),
                |edges| edges.p1_linear_skew_ancestor,
                |node| node.p1_linear_depth
            )
            .await?
            .map(|node| node.cs_id),
        u_level_ancestor.map(name_cs_id)
    );
    Ok(())
}

pub async fn assert_p1_linear_lowest_common_ancestor(
    graph: &CommitGraph,
    ctx: &CoreContext,
    u: &str,
    v: &str,
    lca: Option<&str>,
) -> Result<()> {
    assert_eq!(
        graph
            .lowest_common_ancestor(
                ctx,
                name_cs_id(u),
                name_cs_id(v),
                |edges| edges.parents.first().copied(),
                |edges| edges.p1_linear_skew_ancestor,
                |node| node.p1_linear_depth
            )
            .await?
            .map(|node| node.cs_id),
        lca.map(name_cs_id)
    );
    Ok(())
}
