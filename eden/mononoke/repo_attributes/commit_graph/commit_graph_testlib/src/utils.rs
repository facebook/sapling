/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::RwLock;

use anyhow::Result;
use anyhow::anyhow;
use cloned::cloned;
use commit_graph::AncestorsStreamBuilder;
use commit_graph::BaseCommitGraphWriter;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use commit_graph::LinearAncestorsStreamBuilder;
use commit_graph_types::edges::ChangesetNode;
use commit_graph_types::segments::BoundaryChangesets;
use commit_graph_types::segments::SegmentDescription;
use commit_graph_types::segments::SegmentedSliceDescription;
use context::CoreContext;
use futures::Future;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use itertools::Itertools;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;

use crate::CommitGraphStorageTest;

/// Generate a fake changeset id for graph testing purposes by using the raw
/// bytes of the changeset name, padded with zeroes.
pub fn name_cs_id(name: &str) -> ChangesetId {
    let mut bytes = [0; 32];
    bytes[..name.len()].copy_from_slice(name.as_bytes());
    ChangesetId::from_bytes(bytes).expect("Changeset ID should be valid")
}

pub fn cs_id_name(cs_id: ChangesetId) -> String {
    cs_id
        .to_string()
        .chars()
        .array_chunks::<2>()
        .filter_map(|chunk| match chunk {
            ['0', '0'] => None,
            _ => {
                let chunk = chunk.into_iter().collect::<String>();
                Some(char::from(
                    u8::from_str_radix(&chunk, 16).expect("Changeset Id should come from ASCII"),
                ))
            }
        })
        .collect::<String>()
}

/// Generate a fake changeset node for graph testing purposes by using the raw
/// bytes of the changeset name, padded with zeroes.
pub fn name_cs_node(
    name: &str,
    r#gen: u64,
    skip_tree_depth: u64,
    p1_linear_depth: u64,
) -> ChangesetNode {
    let cs_id = name_cs_id(name);
    let generation = Generation::new(r#gen);
    ChangesetNode {
        cs_id,
        generation,
        subtree_source_generation: generation,
        skip_tree_depth,
        p1_linear_depth,
        subtree_source_depth: skip_tree_depth,
    }
}

/// Build a commit graph from an ASCII-art dag.
pub async fn from_dag(
    ctx: &CoreContext,
    dag: &str,
    storage: Arc<dyn CommitGraphStorageTest>,
) -> Result<CommitGraph> {
    let mut added: BTreeMap<String, ChangesetId> = BTreeMap::new();
    let dag = drawdag::parse(dag);

    let graph = CommitGraph::new(storage);
    let graph_writer = BaseCommitGraphWriter::new(graph.clone());

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
            graph_writer
                .add(ctx, cs_id, parent_ids, Default::default())
                .await?;
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
    storage: &Arc<dyn CommitGraphStorageTest>,
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
    storage: &Arc<dyn CommitGraphStorageTest>,
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

pub async fn assert_ancestors_difference_segment_slices(
    graph: &CommitGraph,
    ctx: &CoreContext,
    heads: &[&str],
    common: &[&str],
    slice_size: u64,
    ancestors_difference_segment_slices: &[&[&str]],
) -> Result<()> {
    let heads = heads.iter().copied().map(name_cs_id).collect();
    let common = common.iter().copied().map(name_cs_id).collect();

    assert_eq!(
        graph
            .ancestors_difference_segment_slices(ctx, heads, common, slice_size)
            .await?
            .try_collect::<Vec<_>>()
            .await?
            .into_iter()
            .map(|slice| { slice.into_iter().map(cs_id_name).collect::<Vec<_>>() })
            .collect::<Vec<_>>(),
        ancestors_difference_segment_slices
            .iter()
            .map(|slice| { slice.iter().map(|s| s.to_string()).collect::<Vec<_>>() })
            .collect::<Vec<_>>()
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
    storage: &Arc<dyn CommitGraphStorageTest>,
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
            .map(|(gen_group, cs_ids)| (
                gen_group.value(),
                cs_ids.into_iter().collect::<HashSet<_>>()
            ))
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

pub async fn assert_segmented_slice_ancestors(
    graph: &CommitGraph,
    ctx: &CoreContext,
    heads: Vec<&str>,
    common: Vec<&str>,
    slice_size: u64,
    expected_slices: Vec<Vec<(&str, &str)>>,
    expected_boundary_changesets: Vec<&str>,
) -> Result<()> {
    let heads = heads.into_iter().map(name_cs_id).collect();
    let common = common.into_iter().map(name_cs_id).collect();

    let expected_slices: Vec<_> = expected_slices
        .into_iter()
        .map(|segments| SegmentedSliceDescription {
            segments: segments
                .into_iter()
                .map(|(head, base)| SegmentDescription {
                    head: name_cs_id(head),
                    base: name_cs_id(base),
                })
                .collect(),
        })
        .collect();
    let expected_boundary_changesets = expected_boundary_changesets
        .into_iter()
        .map(name_cs_id)
        .collect::<BoundaryChangesets>();

    let (slices, boundary_changesets) = graph
        .segmented_slice_ancestors(ctx, heads, common, slice_size)
        .await?;

    assert_eq!(slices, expected_slices);
    assert_eq!(boundary_changesets, expected_boundary_changesets);

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

pub async fn assert_descendants(
    graph: &CommitGraph,
    ctx: &CoreContext,
    cs_ids: Vec<&str>,
    descendants: Vec<&str>,
) -> Result<()> {
    assert_eq!(
        graph
            .descendants(ctx, cs_ids.into_iter().map(name_cs_id).collect())
            .await?
            .into_iter()
            .collect::<HashSet<_>>(),
        descendants
            .into_iter()
            .map(name_cs_id)
            .collect::<HashSet<_>>(),
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

    assert_eq!(
        graph
            .verified_ancestors_difference_segments(ctx, heads, common)
            .await?
            .len(),
        segments_count
    );

    Ok(())
}

pub async fn assert_locations_to_changeset_ids(
    ctx: &CoreContext,
    graph: &CommitGraph,
    cs_id: &str,
    distance: u64,
    count: u64,
    ancestors: Vec<&str>,
) -> Result<()> {
    let cs_id = name_cs_id(cs_id);
    let ancestors = ancestors.into_iter().map(name_cs_id).collect::<Vec<_>>();

    assert_eq!(
        graph
            .locations_to_changeset_ids(ctx, cs_id, distance, count)
            .await?,
        ancestors,
    );

    Ok(())
}

pub async fn assert_locations_to_changeset_ids_errors(
    ctx: &CoreContext,
    graph: &CommitGraph,
    cs_id: &str,
    distance: u64,
    count: u64,
) -> Result<()> {
    let cs_id = name_cs_id(cs_id);

    assert!(
        graph
            .locations_to_changeset_ids(ctx, cs_id, distance, count)
            .await
            .is_err(),
    );

    Ok(())
}

pub async fn assert_changeset_ids_to_locations(
    ctx: &CoreContext,
    graph: &CommitGraph,
    heads: Vec<&str>,
    targets: Vec<&str>,
) -> Result<()> {
    let targets = targets.into_iter().map(name_cs_id).collect::<Vec<_>>();
    let heads = heads.into_iter().map(name_cs_id).collect::<Vec<_>>();

    let locations = graph
        .changeset_ids_to_locations(ctx, heads.clone(), targets.clone())
        .await?;

    for target in targets {
        // If a target is an ancestor of any of the heads a location should be returned,
        // otherwise a location shouldn't be returned.
        if graph.is_ancestor_of_any(ctx, target, heads.clone()).await? {
            let location = locations.get(&target).ok_or_else(|| {
                anyhow!(
                    "changeset_ids_to_locations didn't return location for {} which is an ancestor of heads {:?}", target, heads
                )
            })?;
            // Verify that the returned location resolves to the target.
            assert_eq!(
                graph
                    .locations_to_changeset_ids(ctx, location.cs_id, location.distance, 1)
                    .await?,
                vec![target]
            );
        } else {
            assert!(!locations.contains_key(&target));
        }
    }

    Ok(())
}

pub async fn assert_process_topologically(
    ctx: &CoreContext,
    graph: &CommitGraph,
    cs_ids: Vec<&str>,
) -> Result<()> {
    let cs_ids: Vec<_> = cs_ids.into_iter().map(name_cs_id).collect();

    let processed_order = Arc::new(RwLock::new(vec![]));
    graph
        .process_topologically(ctx, cs_ids.clone(), |cs_id| {
            cloned!(processed_order);
            async move {
                processed_order.write().unwrap().push(cs_id);
                Ok(())
            }
        })
        .await?;

    let mut remaining_cs_ids = cs_ids.into_iter().collect::<HashSet<_>>();

    // Verify that all changesets in `cs_ids` were processed, and that they
    // were processed after all of their parents that are also in `cs_ids`.
    let processed_ordered = Arc::into_inner(processed_order).unwrap().into_inner()?;
    for cs_id in processed_ordered.into_iter() {
        let parents = graph.changeset_parents(ctx, cs_id).await?;
        for parent in parents {
            assert!(!remaining_cs_ids.contains(&parent));
        }
        remaining_cs_ids.remove(&cs_id);
    }
    assert!(remaining_cs_ids.is_empty());

    Ok(())
}

pub async fn assert_minimize_frontier(
    ctx: &CoreContext,
    graph: &CommitGraph,
    cs_ids: Vec<&str>,
    expected_frontier: Vec<&str>,
) -> Result<()> {
    let cs_ids: Vec<_> = cs_ids.into_iter().map(name_cs_id).collect();
    let expected_frontier: Vec<_> = expected_frontier.into_iter().map(name_cs_id).collect();

    let minimal_frontier = graph.minimize_frontier(ctx, cs_ids.clone()).await?;

    assert_eq!(
        minimal_frontier.iter().copied().collect::<HashSet<_>>(),
        expected_frontier.iter().copied().collect::<HashSet<_>>()
    );

    // Check that no two changesets in the minimal frontier are ancestors of each other.
    for cs_id_pair in minimal_frontier.iter().permutations(2) {
        if let &[x, y] = cs_id_pair.as_slice() {
            assert!(!graph.is_ancestor(ctx, *x, *y).await?);
        }
    }

    // Check that the ancestors of the initial frontier and the ancestors of the minimal frontier
    // are equal.
    assert_eq!(
        graph
            .ancestors_difference(ctx, cs_ids, vec![])
            .await?
            .into_iter()
            .collect::<HashSet<_>>(),
        graph
            .ancestors_difference(ctx, minimal_frontier, vec![])
            .await?
            .into_iter()
            .collect::<HashSet<_>>()
    );

    Ok(())
}

pub async fn assert_ancestors_within_distance(
    ctx: &CoreContext,
    graph: &CommitGraph,
    cs_ids: Vec<&str>,
    max_distance: u64,
    expected_ancestors_and_distances: Vec<(&str, u64)>,
) -> Result<()> {
    let cs_ids: Vec<_> = cs_ids.into_iter().map(name_cs_id).collect();
    let ancestors_and_distances = graph
        .ancestors_within_distance_stream(ctx, cs_ids.clone(), max_distance)
        .await?
        .try_collect::<HashSet<_>>()
        .await?;

    let expected_ancestors_and_distances: HashSet<_> = expected_ancestors_and_distances
        .into_iter()
        .map(|(name, distance)| (name_cs_id(name), distance))
        .collect();

    assert_eq!(ancestors_and_distances, expected_ancestors_and_distances);

    let ancestors_and_boundaries = graph
        .ancestors_within_distance(ctx, cs_ids, max_distance)
        .await?;
    let boundaries = ancestors_and_boundaries
        .boundaries
        .clone()
        .into_iter()
        .collect::<HashSet<_>>();
    let all_ancestors = ancestors_and_boundaries
        .ancestors
        .into_iter()
        .chain(ancestors_and_boundaries.boundaries)
        .collect::<HashSet<_>>();

    assert_eq!(
        all_ancestors,
        expected_ancestors_and_distances
            .iter()
            .map(|(cs_id, _)| cs_id)
            .copied()
            .collect::<HashSet<_>>()
    );
    assert_eq!(
        boundaries,
        expected_ancestors_and_distances
            .iter()
            .filter(|(_, distance)| *distance == max_distance)
            .map(|(cs_id, _)| cs_id)
            .copied()
            .collect::<HashSet<_>>()
    );

    Ok(())
}

pub async fn assert_find_boundary(
    ctx: &CoreContext,
    graph: &CommitGraph,
    cs_ids: Vec<&str>,
    expected_boundary: Vec<&str>,
) -> Result<()> {
    let cs_ids: Vec<_> = cs_ids.into_iter().map(name_cs_id).collect();
    let boundary = graph
        .find_boundary(ctx, cs_ids.clone())
        .await?
        .into_iter()
        .map(cs_id_name)
        .collect::<HashSet<_>>();

    let expected_boundary: HashSet<_> = expected_boundary
        .into_iter()
        .map(|cs_name| cs_name.to_string())
        .collect();

    assert_eq!(boundary, expected_boundary);
    Ok(())
}

pub async fn assert_linear_ancestors_stream(
    ctx: &CoreContext,
    graph: &CommitGraph,
    head: &str,
    exclude_ancestors_of: Option<&str>,
    descendants_of: Option<&str>,
    skip: Option<u64>,
    expected_output: Vec<&str>,
) -> Result<()> {
    let head = name_cs_id(head);
    let exclude_ancestors_of = exclude_ancestors_of.map(name_cs_id);
    let descendants_of = descendants_of.map(name_cs_id);
    let expected_output = expected_output
        .into_iter()
        .map(name_cs_id)
        .collect::<Vec<_>>();

    let mut builder =
        LinearAncestorsStreamBuilder::new(Arc::new(graph.clone()), ctx.clone(), head).await?;

    if let Some(exclude_ancestors_of) = exclude_ancestors_of {
        builder = builder.exclude_ancestors_of(exclude_ancestors_of).await?;
    }

    if let Some(descendants_of) = descendants_of {
        builder = builder.descendants_of(descendants_of).await?;
    }

    if let Some(skip) = skip {
        builder = builder.skip(skip);
    }

    let stream = builder.build().await?;
    let ancestors = stream.try_collect::<Vec<_>>().await?;

    assert_eq!(ancestors, expected_output);

    Ok(())
}
