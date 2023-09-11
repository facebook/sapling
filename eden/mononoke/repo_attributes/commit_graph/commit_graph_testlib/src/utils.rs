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
use commit_graph::AncestorsStreamBuilder;
use commit_graph::CommitGraph;
use commit_graph_types::edges::ChangesetNode;
use commit_graph_types::storage::CommitGraphStorage;
use context::CoreContext;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::Future;
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
            .maybe_fetch_edges(ctx, name_cs_id(u))
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
            .maybe_fetch_edges(ctx, name_cs_id(u))
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
            .skip_tree_level_ancestor(ctx, name_cs_id(u), target_depth,)
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
            .skip_tree_lowest_common_ancestor(ctx, name_cs_id(u), name_cs_id(v),)
            .await?
            .map(|node| node.cs_id),
        lca.map(name_cs_id)
    );
    Ok(())
}

pub async fn assert_ancestors_difference_with<Property, Out>(
    graph: &CommitGraph,
    ctx: &CoreContext,
    heads: Vec<&str>,
    common: Vec<&str>,
    property_fn: Property,
    ancestors_difference: Vec<&str>,
) -> Result<()>
where
    Property: Fn(ChangesetId) -> Out + Send + Sync + 'static,
    Out: Future<Output = Result<bool>> + Send + Sync + 'static,
{
    let heads = heads.into_iter().map(name_cs_id).collect();
    let common = common.into_iter().map(name_cs_id).collect();

    assert_eq!(
        AncestorsStreamBuilder::new(Arc::new(graph.clone()), ctx.clone(), heads)
            .exclude_ancestors_of(common)
            .without(property_fn)
            .build()
            .await?
            .try_collect::<HashSet<_>>()
            .await?,
        ancestors_difference
            .into_iter()
            .map(name_cs_id)
            .collect::<HashSet<_>>()
    );
    Ok(())
}

pub async fn assert_ancestors_difference(
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
            .ancestors_difference(ctx, heads, common)
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

pub async fn assert_topological_order(
    graph: &CommitGraph,
    ctx: &CoreContext,
    cs_ids: &Vec<ChangesetId>,
) -> Result<()> {
    let all_cs_ids: HashSet<ChangesetId> = cs_ids.iter().copied().collect();
    let mut previous_cs_ids: HashSet<ChangesetId> = Default::default();

    for cs_id in cs_ids {
        let parents = graph.changeset_parents(ctx, *cs_id).await?;
        // Check that each parent of cs_id either isn't contained in cs_ids
        // or is found before cs_id.
        assert!(
            parents
                .into_iter()
                .all(|parent| !all_cs_ids.contains(&parent) || previous_cs_ids.contains(&parent))
        );
        previous_cs_ids.insert(*cs_id);
    }

    Ok(())
}

pub async fn assert_range_stream(
    graph: &CommitGraph,
    ctx: &CoreContext,
    start: &str,
    end: &str,
    range: Vec<&str>,
) -> Result<()> {
    let start_id = name_cs_id(start);
    let end_id = name_cs_id(end);

    let range_stream_cs_ids = graph
        .range_stream(ctx, start_id, end_id)
        .await?
        .collect::<Vec<_>>()
        .await;

    assert_topological_order(graph, ctx, &range_stream_cs_ids).await?;

    assert_eq!(
        range_stream_cs_ids.into_iter().collect::<HashSet<_>>(),
        range.into_iter().map(name_cs_id).collect::<HashSet<_>>()
    );
    Ok(())
}

pub async fn assert_ancestors_frontier_with<Property, Out>(
    graph: &CommitGraph,
    ctx: &CoreContext,
    heads: Vec<&str>,
    property_fn: Property,
    ancestors_frontier: Vec<&str>,
) -> Result<()>
where
    Property: Fn(ChangesetId) -> Out + Send + Sync + 'static,
    Out: Future<Output = Result<bool>>,
{
    let heads = heads.into_iter().map(name_cs_id).collect();

    assert_eq!(
        graph
            .ancestors_frontier_with(ctx, heads, property_fn)
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
            .maybe_fetch_edges(ctx, name_cs_id(u))
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
            .p1_linear_level_ancestor(ctx, name_cs_id(u), target_depth)
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
            .p1_linear_lowest_common_ancestor(ctx, name_cs_id(u), name_cs_id(v))
            .await?
            .map(|node| node.cs_id),
        lca.map(name_cs_id)
    );
    Ok(())
}

pub async fn assert_common_base(
    graph: &CommitGraph,
    ctx: &CoreContext,
    u: &str,
    v: &str,
    common_base: Vec<&str>,
) -> Result<()> {
    assert_eq!(
        graph
            .common_base(ctx, name_cs_id(u), name_cs_id(v))
            .await?
            .into_iter()
            .collect::<HashSet<_>>(),
        common_base
            .into_iter()
            .map(name_cs_id)
            .collect::<HashSet<_>>()
    );
    Ok(())
}

pub async fn assert_slice_ancestors<NeedsProcessing, Out>(
    graph: &CommitGraph,
    ctx: &CoreContext,
    heads: Vec<&str>,
    needs_processing: NeedsProcessing,
    slice_size: u64,
    slices: Vec<(u64, Vec<&str>)>,
) -> Result<()>
where
    NeedsProcessing: Fn(Vec<ChangesetId>) -> Out,
    Out: Future<Output = Result<HashSet<ChangesetId>>>,
{
    let heads = heads.into_iter().map(name_cs_id).collect();
    assert_eq!(
        graph
            .slice_ancestors(ctx, heads, needs_processing, slice_size)
            .await?
            .into_iter()
            .map(|(gen_group, cs_ids)| (gen_group, cs_ids.into_iter().collect::<HashSet<_>>()))
            .collect::<Vec<_>>(),
        slices
            .into_iter()
            .map(|(gen_group, cs_ids)| (
                gen_group,
                cs_ids.into_iter().map(name_cs_id).collect::<HashSet<_>>()
            ))
            .collect::<Vec<_>>(),
    );
    Ok(())
}

pub async fn assert_children(
    graph: &CommitGraph,
    ctx: &CoreContext,
    cs_id: &str,
    children: Vec<&str>,
) -> Result<()> {
    assert_eq!(
        graph
            .changeset_children(ctx, name_cs_id(cs_id))
            .await?
            .into_iter()
            .collect::<HashSet<_>>(),
        children.into_iter().map(name_cs_id).collect::<HashSet<_>>(),
    );
    Ok(())
}

pub async fn assert_ancestors_difference_segments(
    ctx: &CoreContext,
    graph: &CommitGraph,
    heads: Vec<&str>,
    common: Vec<&str>,
    segments_count: usize,
) -> Result<()> {
    let heads: Vec<_> = heads.into_iter().map(name_cs_id).collect();
    let common: Vec<_> = common.into_iter().map(name_cs_id).collect();

    assert!(
        graph
            .verified_ancestors_difference_segments(ctx, heads, common)
            .await?
            .len()
            == segments_count
    );

    Ok(())
}
