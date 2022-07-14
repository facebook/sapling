/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use reloader::Loader;
use reloader::Reloader;
use std::cmp::min;
use std::collections::HashMap;
use std::collections::HashSet;
use std::num::NonZeroI64;
use std::sync::Arc;

use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use context::PerfCounterType;
use dashmap::DashMap;
use futures::future::try_join_all;
use futures::stream::futures_unordered::FuturesUnordered;
use futures::stream::TryStreamExt;
use futures_util::try_join;
use maplit::hashmap;
use maplit::hashset;
use slog::info;
use slog::Logger;
use tokio::task;

use changeset_fetcher::ArcChangesetFetcher;
use changeset_fetcher::ChangesetFetcher;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;
use mononoke_types::FIRST_GENERATION;

use common::advance_bfs_layer;
use common::changesets_with_generation_numbers;
use common::check_if_node_exists;
use common::fetch_generation;
use common::get_parents;
use reachabilityindex::errors::*;
use reachabilityindex::LeastCommonAncestorsHint;
use reachabilityindex::NodeFrontier;
use reachabilityindex::ReachabilityIndex;

use fbthrift::compact_protocol;

pub mod sparse;

const DEFAULT_EDGE_COUNT: u32 = 10;

// Each indexed node fits into one of two categories:
// - It has skiplist edges
// - It only has edges to its parents.
#[derive(Clone, Debug, PartialEq)]
pub enum SkiplistNodeType {
    SingleEdge((ChangesetId, Generation)),
    // A list of skip edges which keep doubling
    // in distance from their root node.
    // The ith skip edge is at most 2^i commits away.
    SkipEdges(Vec<(ChangesetId, Generation)>),
    ParentEdges(Vec<(ChangesetId, Generation)>),
}

impl SkiplistNodeType {
    pub fn to_thrift(&self) -> skiplist_thrift::SkiplistNodeType {
        fn encode_edge_to_thrift(
            cs_id: ChangesetId,
            gen_num: Generation,
        ) -> skiplist_thrift::CommitAndGenerationNumber {
            let cs_id = cs_id.into_thrift();
            let gen = skiplist_thrift::GenerationNum(gen_num.value() as i64);
            skiplist_thrift::CommitAndGenerationNumber { cs_id, gen }
        }

        fn encode_vec_to_thrift(
            cs_gen: Vec<(ChangesetId, Generation)>,
        ) -> Vec<skiplist_thrift::CommitAndGenerationNumber> {
            cs_gen
                .into_iter()
                .map(|(cs_id, gen_num)| encode_edge_to_thrift(cs_id, gen_num))
                .collect()
        }

        match self {
            SkiplistNodeType::SingleEdge((cs_id, gen)) => {
                let edges = vec![encode_edge_to_thrift(*cs_id, *gen)];
                let skip_edges = skiplist_thrift::SkipEdges { edges };
                skiplist_thrift::SkiplistNodeType::SkipEdges(skip_edges)
            }
            SkiplistNodeType::SkipEdges(edges) => {
                let edges = encode_vec_to_thrift(edges.clone());
                let skip_edges = skiplist_thrift::SkipEdges { edges };
                skiplist_thrift::SkiplistNodeType::SkipEdges(skip_edges)
            }
            SkiplistNodeType::ParentEdges(parent_edges) => {
                let edges = encode_vec_to_thrift(parent_edges.clone());
                let parent_edges = skiplist_thrift::ParentEdges { edges };
                skiplist_thrift::SkiplistNodeType::ParentEdges(parent_edges)
            }
        }
    }

    pub fn from_thrift(skiplist_node: skiplist_thrift::SkiplistNodeType) -> Result<Self> {
        fn decode_vec_to_thrift(
            edges: Vec<skiplist_thrift::CommitAndGenerationNumber>,
        ) -> Result<Vec<(ChangesetId, Generation)>> {
            edges
                .into_iter()
                .map(|commit_gen_num| {
                    let cs_id = commit_gen_num.cs_id;
                    let gen_num = commit_gen_num.gen;
                    ChangesetId::from_thrift(cs_id)
                        .map(|cs_id| (cs_id, Generation::new(gen_num.0 as u64)))
                })
                .collect()
        }

        match skiplist_node {
            skiplist_thrift::SkiplistNodeType::SkipEdges(thrift_edges) => {
                decode_vec_to_thrift(thrift_edges.edges).map(|edges| {
                    if edges.len() == 1 {
                        SkiplistNodeType::SingleEdge(edges[0])
                    } else {
                        SkiplistNodeType::SkipEdges(edges)
                    }
                })
            }
            skiplist_thrift::SkiplistNodeType::ParentEdges(thrift_edges) => {
                decode_vec_to_thrift(thrift_edges.edges).map(SkiplistNodeType::ParentEdges)
            }
            _ => Err(ErrorKind::UknownSkiplistThriftEncoding.into()),
        }
    }

    pub fn serialize(&self) -> Bytes {
        let thrift_skiplist_node_type = self.to_thrift();
        compact_protocol::serialize(&thrift_skiplist_node_type)
    }
}

pub fn deserialize_skiplist_index(logger: Logger, bytes: Bytes) -> Result<SkiplistIndex> {
    deserialize_skiplist_mapping(logger, bytes).map(SkiplistIndex::from_edges)
}

fn deserialize_skiplist_mapping(logger: Logger, bytes: Bytes) -> Result<SkiplistEdgeMapping> {
    let map: HashMap<_, skiplist_thrift::SkiplistNodeType> = compact_protocol::deserialize(bytes)?;
    let cmap: DashMap<ChangesetId, SkiplistNodeType> = DashMap::with_capacity(map.len());
    let mut pnodecount = 0;
    let mut snodecount = 0;
    let mut maxsedgelen = 0;
    let mut maxpedgelen = 0;
    for (cs_id, skiplist_thrift) in map {
        let v = SkiplistNodeType::from_thrift(skiplist_thrift)?;
        match &v {
            SkiplistNodeType::SingleEdge(_) => {
                snodecount += 1;
                if 1 > maxsedgelen {
                    maxsedgelen = 1;
                }
            }
            SkiplistNodeType::SkipEdges(edges) => {
                let sedgelen = edges.len();
                if sedgelen > maxsedgelen {
                    maxsedgelen = sedgelen;
                }
                snodecount += sedgelen;
            }
            SkiplistNodeType::ParentEdges(edges) => {
                let edgelen = edges.len();
                if edgelen > maxpedgelen {
                    maxpedgelen = edgelen;
                }
                pnodecount += edges.len()
            }
        }
        cmap.insert(ChangesetId::from_thrift(cs_id)?, v);
    }
    info!(
        logger,
        "cmap size {}, parent nodecount {}, skip nodecount {}, maxsedgelen {}, maxpedgelen {}",
        cmap.len(),
        pnodecount,
        snodecount,
        maxsedgelen,
        maxpedgelen
    );
    Ok(SkiplistEdgeMapping::from_map(cmap))
}

#[derive(Debug, Clone)]
struct SkiplistEdgeMapping {
    pub mapping: DashMap<ChangesetId, SkiplistNodeType>,
    pub skip_edges_per_node: u32,
}

impl SkiplistEdgeMapping {
    pub fn new() -> Self {
        SkiplistEdgeMapping {
            mapping: DashMap::new(),
            skip_edges_per_node: DEFAULT_EDGE_COUNT,
        }
    }

    pub fn from_map(map: DashMap<ChangesetId, SkiplistNodeType>) -> Self {
        SkiplistEdgeMapping {
            mapping: map,
            skip_edges_per_node: DEFAULT_EDGE_COUNT,
        }
    }

    pub fn with_skip_edge_count(self, skip_edges_per_node: u32) -> Self {
        SkiplistEdgeMapping {
            skip_edges_per_node,
            ..self
        }
    }
}

/// helper function that computes a single skip edge by leveraging the existing skiplist
/// without assuming its completeness.
async fn compute_single_skip_edge(
    ctx: &CoreContext,
    changeset_fetcher: &ArcChangesetFetcher,
    skip_list_edges: &Arc<SkiplistEdgeMapping>,
    (node, gen): (ChangesetId, Generation),
    target_gen: Generation,
) -> Result<ChangesetId, Error> {
    let initial_frontier = hashmap! {gen => hashset!{node}};
    let initial_frontier = NodeFrontier::new(initial_frontier);
    let target_frontier = process_frontier(
        ctx,
        changeset_fetcher,
        skip_list_edges,
        initial_frontier,
        target_gen,
        &None,
    )
    .await?;

    let target_changeset = target_frontier
        .get(&target_gen)
        .ok_or(ErrorKind::ProgrammingError(
            "frontier doesn't have target generation",
        ))?
        .iter()
        .next()
        .ok_or(ErrorKind::ProgrammingError("inconsistent frontier state"))?;
    Ok(*target_changeset)
}

fn nth_node_or_last<T: Clone>(v: &Vec<T>, i: usize) -> Option<T> {
    return v.get(i).or(v.last()).cloned();
}

/// compute the skip list edges which start by pointing at start_node.
/// goes as far as possible, if an unindexed or merge node are reached then skip edges
/// will not go past that node.
/// note that start node is not the node that we'll be adding these skip list edges to.
/// it is the first node that we'll consider as a candidate for a skip list edge.
/// hence it should always be the parent of the node we are creating edges for.
///
/// Because we sometimes trim the skiplist structure to just have the single entry to save space we
/// can't assume all the edges being present in ancestor's entries.  In those cases (when the
/// ancestor skip pointers point to far to be useful) we need to manually compute the targets of
/// skip pointers instead of trusting the data from ancestor's skiplits.
async fn compute_skip_edges(
    ctx: CoreContext,
    changeset_fetcher: ArcChangesetFetcher,
    start_node: (ChangesetId, Generation),
    skip_edge_mapping: Arc<SkiplistEdgeMapping>,
) -> Result<Vec<(ChangesetId, Generation)>, Error> {
    let source_gen = start_node.1.add(1);
    let mut curr = start_node;

    let max_skip_edge_count = skip_edge_mapping.skip_edges_per_node as usize;
    let mut skip_edges = vec![curr];
    let mut i: usize = 0;

    loop {
        // edge in ith should point no further than 2^(i+1) commits back
        let target_gen = source_gen
            .checked_sub(1 << (i + 1))
            .unwrap_or(FIRST_GENERATION);
        {
            match skip_edge_mapping.mapping.get(&curr.0) {
                Some(read_locked_entry) => {
                    // ith edge should point not further than 2^(i+1) commits back

                    match &*read_locked_entry {
                        SkiplistNodeType::SingleEdge(next_node) => {
                            curr = *next_node;
                        }
                        SkiplistNodeType::SkipEdges(edges) => {
                            if let Some(next_node) = nth_node_or_last(edges, i) {
                                curr = next_node;
                            } else {
                                break;
                            }
                        }
                        _ => break,
                    };
                }
                None => {
                    break;
                }
            }
        }
        if target_gen > curr.1 {
            // the pointer is pointing too far - weed need to fixup
            curr = (
                compute_single_skip_edge(
                    &ctx,
                    &changeset_fetcher,
                    &skip_edge_mapping,
                    start_node,
                    target_gen,
                )
                .await?,
                target_gen,
            );
        }
        skip_edges.push(curr);
        if skip_edges.len() >= max_skip_edge_count {
            break;
        }
        i += 1;
    }
    Ok(skip_edges)
}
/// Structure for indexing skip list edges for reachability queries.
#[facet::facet]
#[derive(Debug, Clone)]
pub struct SkiplistIndex {
    // Each hash that the structure knows about is mapped to a  collection
    // of (Gen, Hash) pairs, wrapped in an enum. The semantics behind this are:
    // - If the hash isn't in the hash map, the node hasn't been indexed yet.
    // - If the enum type is SkipEdges, then we can safely traverse the longest
    //   edge that doesn't pass the generation number of the destination.
    // - If the enum type is ParentEdges, then we couldn't safely add skip edges
    //   from this node (which is always the case for a merge node), so we must
    //   recurse on all the children.
    skip_list_edges: Reloader<SkiplistEdgeMapping>,
}

// Find nodes to index during lazy indexing
// This method searches backwards from a start node until a specified depth,
// collecting all nodes which are not currently present in the index.
// Then it orders them topologically using their generation numbers and returns them.
async fn find_nodes_to_index(
    ctx: &CoreContext,
    changeset_fetcher: &ArcChangesetFetcher,
    skip_list_edges: &Arc<SkiplistEdgeMapping>,
    (start_node, start_gen): (ChangesetId, Generation),
    depth: u64,
) -> Result<Vec<(ChangesetId, Generation)>, Error> {
    let mut bfs_layer: HashSet<_> = vec![(start_node, start_gen)].into_iter().collect();
    let mut seen: HashSet<_> = HashSet::new();
    let mut curr_depth = depth;

    check_if_node_exists(ctx, changeset_fetcher, start_node).await?;
    loop {
        bfs_layer = bfs_layer
            .into_iter()
            .filter(|(hash, _gen)| !skip_list_edges.mapping.contains_key(hash))
            .collect();

        if curr_depth == 0 || bfs_layer.is_empty() {
            break;
        } else {
            let (next_bfs_layer, next_seen) =
                advance_bfs_layer(ctx, changeset_fetcher, bfs_layer, seen).await?;
            bfs_layer = next_bfs_layer;
            seen = next_seen;
            curr_depth -= 1;
        }
    }

    let mut top_order = seen.into_iter().collect::<Vec<_>>();
    top_order.sort_by(|a, b| (a.1).cmp(&b.1));
    Ok(top_order)
}

/// From a starting node, index all nodes that are reachable within a given distance.
/// If a previously indexed node is reached, indexing will stop there.
async fn lazy_index_node(
    ctx: &CoreContext,
    changeset_fetcher: &ArcChangesetFetcher,
    skip_edge_mapping: &Arc<SkiplistEdgeMapping>,
    node: ChangesetId,
    max_depth: u64,
) -> Result<(), Error> {
    // if this node is indexed or we've passed the max depth, return
    if max_depth == 0 || skip_edge_mapping.mapping.contains_key(&node) {
        return Ok(());
    }

    let gen = fetch_generation(ctx, changeset_fetcher, node).await?;
    let node_gen_pairs = find_nodes_to_index(
        ctx,
        changeset_fetcher,
        skip_edge_mapping,
        (node, gen),
        max_depth,
    )
    .await?;
    let hash_parentgens_gen_vec =
        try_join_all(node_gen_pairs.into_iter().map(|(hash, _gen)| async move {
            let parents = get_parents(ctx, changeset_fetcher, hash).await?;
            let parent_gen_pairs =
                changesets_with_generation_numbers(ctx, changeset_fetcher, parents).await?;
            let res: Result<_, Error> = Ok((hash, parent_gen_pairs));
            res
        }))
        .await?;

    for (curr_hash, parent_gen_pairs) in hash_parentgens_gen_vec.into_iter() {
        if parent_gen_pairs.len() != 1 {
            // Merge node or parentless node
            // Reflect this in the index
            skip_edge_mapping
                .mapping
                .insert(curr_hash, SkiplistNodeType::ParentEdges(parent_gen_pairs));
        } else {
            // Single parent node
            // Compute skip edges assuming a reasonable number of parents are indexed.
            // Even if this reaches a non indexed node during,
            // indexing correctness isn't affected.
            let unique_parent_gen_pair = parent_gen_pairs.get(0).unwrap().clone();
            let new_edges = compute_skip_edges(
                ctx.clone(),
                changeset_fetcher.clone(),
                unique_parent_gen_pair,
                skip_edge_mapping.clone(),
            )
            .await?;
            skip_edge_mapping
                .mapping
                .insert(curr_hash, SkiplistNodeType::SkipEdges(new_edges));
        }
    }
    Ok(())
}

struct SkiplistLoader {
    ctx: CoreContext,
    blobstore_key: String,
    blobstore_without_cache: Arc<dyn Blobstore>,
}

#[async_trait]
impl Loader<SkiplistEdgeMapping> for SkiplistLoader {
    async fn load(&mut self) -> Result<Option<SkiplistEdgeMapping>> {
        if tunables::tunables().get_skiplist_reload_disabled() {
            return Ok(None);
        }
        info!(self.ctx.logger(), "Fetching skiplist");
        let mapping_fut = task::spawn({
            cloned!(self.ctx, self.blobstore_without_cache, self.blobstore_key);
            async move {
                let maybebytes = blobstore_without_cache.get(&ctx, &blobstore_key).await?;
                match maybebytes {
                    Some(bytes) => {
                        let bytes = bytes.into_raw_bytes();
                        let logger = ctx.logger().clone();
                        let mapping = task::spawn_blocking(move || {
                            deserialize_skiplist_mapping(logger, bytes)
                        })
                        .await??;
                        info!(ctx.logger(), "Built skiplist");
                        Ok(Some(mapping))
                    }
                    None => {
                        info!(ctx.logger(), "Skiplist is empty!");
                        Ok(Some(SkiplistEdgeMapping::new()))
                    }
                }
            }
        });

        mapping_fut.await?
    }
}

impl SkiplistIndex {
    pub fn new() -> Self {
        Self::from_edges(SkiplistEdgeMapping::new())
    }

    fn from_edges(mapping: SkiplistEdgeMapping) -> Self {
        Self {
            skip_list_edges: Reloader::fixed(mapping),
        }
    }

    pub async fn from_blobstore(
        ctx: &CoreContext,
        maybe_blobstore_key: &Option<String>,
        blobstore_without_cache: &Arc<dyn Blobstore>,
    ) -> Result<Arc<Self>> {
        match maybe_blobstore_key {
            Some(blobstore_key) => {
                cloned!(ctx, blobstore_key, blobstore_without_cache);
                let loader = SkiplistLoader {
                    ctx: ctx.clone(),
                    blobstore_key,
                    blobstore_without_cache,
                };
                let tunables = tunables::tunables();
                let reloader = Reloader::reload_periodically(
                    ctx.clone(),
                    move || {
                        std::time::Duration::from_secs(
                            NonZeroI64::new(tunables.get_skiplist_reload_interval())
                                .and_then(|n| u64::try_from(n.get()).ok())
                                .unwrap_or(60 * 15),
                        )
                    },
                    loader,
                )
                .await?;
                Ok(Arc::new(Self {
                    skip_list_edges: reloader,
                }))
            }
            None => Ok(Arc::new(SkiplistIndex::new())),
        }
    }

    pub fn new_with_skiplist_graph(skiplist_graph: DashMap<ChangesetId, SkiplistNodeType>) -> Self {
        SkiplistIndex::from_edges(SkiplistEdgeMapping::from_map(skiplist_graph))
    }

    pub fn with_skip_edge_count(skip_edges_per_node: u32) -> Self {
        SkiplistIndex::from_edges(
            SkiplistEdgeMapping::new().with_skip_edge_count(skip_edges_per_node),
        )
    }

    pub fn skip_edge_count(&self) -> u32 {
        self.edges().skip_edges_per_node
    }

    pub async fn add_node(
        &self,
        ctx: &CoreContext,
        changeset_fetcher: &ArcChangesetFetcher,
        node: ChangesetId,
        max_index_depth: u64,
    ) -> Result<(), Error> {
        lazy_index_node(
            ctx,
            changeset_fetcher,
            &self.skip_list_edges.load(),
            node,
            max_index_depth,
        )
        .await
    }

    /// get skiplist edges originating from a particular node hash
    /// returns Some(edges) if this node was indexed with skip edges
    /// returns None if this node was unindexed, or was indexed with parent edges only.
    pub fn get_skip_edges(&self, node: ChangesetId) -> Option<Vec<(ChangesetId, Generation)>> {
        if let Some(read_guard) = self.edges().mapping.get(&node) {
            if let SkiplistNodeType::SkipEdges(edges) = &*read_guard {
                Some(edges.clone())
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Returns the changesets that are the furthest distance from the
    /// originating changeset.
    pub fn get_furthest_edges(&self, node: ChangesetId) -> Option<Vec<(ChangesetId, Generation)>> {
        if let Some(read_guard) = self.edges().mapping.get(&node) {
            match &*read_guard {
                SkiplistNodeType::SingleEdge(edge) => Some(vec![edge.clone()]),
                SkiplistNodeType::SkipEdges(edges) => {
                    Some(edges.last().into_iter().cloned().collect())
                }
                SkiplistNodeType::ParentEdges(edges) => Some(edges.clone()),
            }
        } else {
            None
        }
    }

    /// Returns true if there are any skip edges originating from changesets
    /// in a node frontier.
    pub fn has_any_skip_edges(&self, node_frontier: &NodeFrontier) -> bool {
        let skip_list_edges = self.edges();
        node_frontier.iter().any(|(changeset, _)| {
            if let Some(read_guard) = skip_list_edges.mapping.get(changeset) {
                if let SkiplistNodeType::SkipEdges(_) = &*read_guard {
                    true
                } else {
                    false
                }
            } else {
                false
            }
        })
    }

    fn edges(&self) -> arc_swap::Guard<Arc<SkiplistEdgeMapping>> {
        self.skip_list_edges.load()
    }

    pub fn get_all_skip_edges(&self) -> HashMap<ChangesetId, SkiplistNodeType> {
        self.edges().mapping.clone().into_iter().collect()
    }

    pub fn is_node_indexed(&self, node: ChangesetId) -> bool {
        self.edges().mapping.contains_key(&node)
    }

    pub fn indexed_node_count(&self) -> usize {
        self.edges().mapping.len()
    }

    // Remove all but latest skip entry (i.e. entry with the longest jump) to save space.
    pub fn trim_to_single_entry_per_changeset(&self) {
        let skip_list_edges = self.edges();
        for (cs_id, old_node) in skip_list_edges.mapping.clone().into_iter() {
            let new_node = if let SkiplistNodeType::SkipEdges(skip_edges) = old_node {
                SkiplistNodeType::SkipEdges(skip_edges.last().cloned().into_iter().collect())
            } else {
                old_node
            };
            let _old_node = skip_list_edges.mapping.insert(cs_id, new_node);
        }
    }
}

#[async_trait]
impl ReachabilityIndex for SkiplistIndex {
    async fn query_reachability(
        &self,
        ctx: &CoreContext,
        changeset_fetcher: &ArcChangesetFetcher,
        desc_hash: ChangesetId,
        anc_hash: ChangesetId,
    ) -> Result<bool, Error> {
        let (anc_gen, desc_gen) = try_join!(
            changeset_fetcher.get_generation_number(ctx.clone(), anc_hash),
            changeset_fetcher.get_generation_number(ctx.clone(), desc_hash),
        )?;
        if anc_gen > desc_gen {
            return Ok(false);
        }
        ctx.perf_counters()
            .set_counter(PerfCounterType::SkiplistAncestorGen, anc_gen.value() as i64);
        ctx.perf_counters().set_counter(
            PerfCounterType::SkiplistDescendantGen,
            desc_gen.value() as i64,
        );
        let frontier = process_frontier(
            ctx,
            changeset_fetcher,
            &self.skip_list_edges.load(),
            NodeFrontier::new(hashmap! {desc_gen => hashset!{desc_hash}}),
            anc_gen,
            &None,
        )
        .await?;
        match frontier.get_all_changesets_for_gen_num(anc_gen) {
            Some(cs_ids) => Ok(cs_ids.contains(&anc_hash)),
            None => Ok(false),
        }
    }
}

/// A structure to hold all the visited skiplist edges during a single
/// traversal in a "reverse" mapping: ancestor -> (child, is_child_a_merge_commit)
/// Such structure allows to traverse the graph from ancestor to descendants.
///
/// The merge-commit bit is the information we use for finding merges later without consulting the
/// commit graph.
struct SkiplistTraversalTrace(DashMap<ChangesetId, Vec<(ChangesetId, bool)>>);

impl SkiplistTraversalTrace {
    pub fn new() -> Self {
        SkiplistTraversalTrace(DashMap::new())
    }

    pub fn inner(&self) -> &DashMap<ChangesetId, Vec<(ChangesetId, bool)>> {
        &self.0
    }

    pub fn add(&self, ancestor: ChangesetId, child: (ChangesetId, bool)) {
        self.0
            .entry(ancestor)
            .and_modify(|old_val| {
                old_val.push(child);
            })
            .or_insert_with(|| vec![child]);
    }
}

// Take all changesets from `all_cs_ids` that have skiplist edges in `skip_edges` and moves them.
// Returns changesets that wasn't moved and a NodeFrontier of moved nodes
fn move_skippable_nodes(
    skip_edges: Arc<SkiplistEdgeMapping>,
    all_cs_ids: Vec<ChangesetId>,
    gen: Generation,
    trace: &Option<&SkiplistTraversalTrace>,
) -> (Vec<ChangesetId>, NodeFrontier) {
    let mut no_skiplist_edges = vec![];
    let mut node_frontier = NodeFrontier::default();

    for cs_id in all_cs_ids {
        if let Some(read_locked_entry) = skip_edges.mapping.get(&cs_id) {
            match &*read_locked_entry {
                SkiplistNodeType::SingleEdge(edge_pair) => {
                    if edge_pair.1 >= gen {
                        node_frontier.insert(edge_pair.clone());
                        if let Some(trace) = trace {
                            trace.add(edge_pair.0, (cs_id, false))
                        }
                    } else {
                        no_skiplist_edges.push(cs_id);
                    }
                }
                SkiplistNodeType::SkipEdges(edges) => {
                    let best_edge = edges
                        .iter()
                        .take_while(|edge_pair| edge_pair.1 >= gen)
                        .last()
                        .cloned();
                    if let Some(edge_pair) = best_edge {
                        node_frontier.insert(edge_pair);
                        if let Some(trace) = trace {
                            trace.add(edge_pair.0, (cs_id, false))
                        }
                    } else {
                        no_skiplist_edges.push(cs_id);
                    }
                }
                SkiplistNodeType::ParentEdges(edges) => {
                    for edge_pair in edges {
                        node_frontier.insert(*edge_pair);
                        if let Some(trace) = trace {
                            trace.add(edge_pair.0, (cs_id, edges.len() > 1))
                        }
                    }
                }
            }
        } else {
            no_skiplist_edges.push(cs_id);
        }
    }

    (no_skiplist_edges, node_frontier)
}

// Take all changesets in `cs_ids` and returns a list of their parents with
// their generations.
async fn move_nonskippable_nodes(
    ctx: &CoreContext,
    changeset_fetcher: &ArcChangesetFetcher,
    cs_ids: Vec<ChangesetId>,
    trace: &Option<&SkiplistTraversalTrace>,
) -> Result<Vec<(ChangesetId, Generation)>, Error> {
    let changeset_parent_gen = cs_ids
        .into_iter()
        .map(|cs_id| async move {
            Ok::<_, Error>((cs_id, get_parents(ctx, changeset_fetcher, cs_id).await?))
        })
        .collect::<FuturesUnordered<_>>()
        .and_then(|(cs_id, parents)| async move {
            let parent_gens = try_join_all(parents.into_iter().map(|p| async move {
                let gen = fetch_generation(ctx, changeset_fetcher, p).await?;
                Ok::<_, Error>((p, gen))
            }))
            .await?;
            Ok((cs_id, parent_gens))
        })
        .try_collect::<Vec<_>>()
        .await?;

    if let Some(trace) = trace {
        for (cs_id, parent_gen) in changeset_parent_gen.iter() {
            for (p, _gen) in parent_gen {
                trace.add(*p, (*cs_id, parent_gen.len() > 1));
            }
        }
    }
    Ok(changeset_parent_gen
        .into_iter()
        .flat_map(|(_cs_id, parent_gens)| parent_gens)
        .collect())
}

/// Advances the node frontier towards the target generation by a single conceptual step.
///
/// For an input frontier IN returns: advanceed frontier OUT a number "N" (skip
/// size) that satisfy the following conditions:
///
/// - Max generation number in OUT + N is the max generation in frontier IN
/// - Any ancestor of IN with generation <= "target_gen" is also an ancestor of OUT
///
/// As long as the max_gen in IN > max_gen,  N is greater than 0
async fn process_frontier_single_skip(
    ctx: &CoreContext,
    changeset_fetcher: &ArcChangesetFetcher,
    skip_edges: &Arc<SkiplistEdgeMapping>,
    mut node_frontier: NodeFrontier,
    target_gen: Generation,
    trace: &Option<&SkiplistTraversalTrace>,
) -> Result<(NodeFrontier, u64), Error> {
    let old_max_gen = if let Some(val) = node_frontier.max_gen() {
        if val <= target_gen {
            return Ok((node_frontier, 0));
        }
        val
    } else {
        return Err(ErrorKind::ProgrammingError("frontier can't be empty").into());
    };

    let (_, all_cs_ids) = node_frontier
        .remove_max_gen()
        .ok_or(ErrorKind::ProgrammingError("frontier can't be empty"))?;
    let (no_skiplist_edges, skipped_frontier) = move_skippable_nodes(
        skip_edges.clone(),
        all_cs_ids.into_iter().collect(),
        target_gen,
        trace,
    );
    if skipped_frontier.is_empty() {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SkiplistNoskipIterations);
    } else {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SkiplistSkipIterations);
        if let Some(new) = skipped_frontier.max_gen() {
            ctx.perf_counters().add_to_counter(
                PerfCounterType::SkiplistSkippedGenerations,
                (old_max_gen.value() - new.value()) as i64,
            );
        }
    }

    let gen_cs = move_nonskippable_nodes(ctx, changeset_fetcher, no_skiplist_edges, trace)
        .await?
        .into_iter();

    node_frontier.extend(gen_cs);

    for (gen, s) in skipped_frontier {
        for entry in s {
            node_frontier.insert((entry, gen));
        }
    }
    let new_max_gen = node_frontier
        .max_gen()
        .ok_or(ErrorKind::ProgrammingError("frontier can't be empty"))?;
    Ok((node_frontier, old_max_gen.value() - new_max_gen.value()))
}

/// Returns a frontier "C" that satisfy the following condition:
/// - Max generation number in "C" is <= "max_gen"
/// - Any ancestor of "node_frontier" with generation <= "max_gen" is also an ancestor of "C"
async fn process_frontier(
    ctx: &CoreContext,
    changeset_fetcher: &ArcChangesetFetcher,
    skip_edges: &Arc<SkiplistEdgeMapping>,
    node_frontier: NodeFrontier,
    max_gen: Generation,
    trace: &Option<&SkiplistTraversalTrace>,
) -> Result<NodeFrontier, Error> {
    let max_skips_without_yield = tunables::tunables().get_skiplist_max_skips_without_yield();
    let mut skips_without_yield = 0;
    let mut node_frontier = node_frontier;

    loop {
        let (new_node_frontier, step_size) = process_frontier_single_skip(
            ctx,
            changeset_fetcher,
            skip_edges,
            node_frontier,
            max_gen,
            trace,
        )
        .await?;
        node_frontier = new_node_frontier;
        if step_size == 0 {
            break;
        }
        if let Some(val) = node_frontier.max_gen() {
            if val <= max_gen {
                break;
            }
        }
        skips_without_yield += 1;
        if max_skips_without_yield != 0 && skips_without_yield >= max_skips_without_yield {
            tokio::task::yield_now().await;
            skips_without_yield = 0;
        }
    }
    Ok(node_frontier)
}

#[async_trait]
impl LeastCommonAncestorsHint for SkiplistIndex {
    async fn lca_hint(
        &self,
        ctx: &CoreContext,
        changeset_fetcher: &ArcChangesetFetcher,
        node_frontier: NodeFrontier,
        gen: Generation,
    ) -> Result<NodeFrontier, Error> {
        process_frontier(
            ctx,
            changeset_fetcher,
            &self.skip_list_edges.load(),
            node_frontier,
            gen,
            &None,
        )
        .await
    }

    /// Check if `ancestor` changeset is an ancestor of `descendant` changeset
    /// Note that a changeset IS NOT its own ancestor
    async fn is_ancestor(
        &self,
        ctx: &CoreContext,
        changeset_fetcher: &ArcChangesetFetcher,
        ancestor: ChangesetId,
        descendant: ChangesetId,
    ) -> Result<bool, Error> {
        if ancestor == descendant {
            Ok(false)
        } else {
            self.query_reachability(ctx, changeset_fetcher, descendant, ancestor)
                .await
        }
    }
}

impl SkiplistIndex {
    /// Helper for moving two frontiers in-sync which we do a lot for LCA computations.
    async fn process_frontiers(
        &self,
        ctx: &CoreContext,
        changeset_fetcher: &ArcChangesetFetcher,
        frontier1: &NodeFrontier,
        frontier2: &NodeFrontier,
        gen: Generation,
    ) -> Result<(NodeFrontier, NodeFrontier), Error> {
        try_join!(
            self.lca_hint(ctx, changeset_fetcher, frontier1.clone(), gen,),
            self.lca_hint(ctx, changeset_fetcher, frontier2.clone(), gen,),
        )
    }

    /// Return lowest common ancestor of two changesets. In graphs with merges,
    /// where there might be more than one such ancestor, this function is guaranteed to
    /// return all the common ancestors with highest generation number.
    pub async fn lca(
        &self,
        ctx: CoreContext,
        changeset_fetcher: ArcChangesetFetcher,
        node1: ChangesetId,
        node2: ChangesetId,
    ) -> Result<Vec<ChangesetId>, Error> {
        // When using skiplists we'll be only using the maximum skip size as in
        // practice that's the only skip that's present.
        let skip_step_size: u64 = 1 << (self.edges().skip_edges_per_node - 1);

        // STAGE 1: look for any common ancestor (not neccesarily lowest) while using
        // skiplists as much as possible.

        // Invariant:
        // lca(node1, node2) == ∑ lca(nodef1, nodef2)
        //                      ^(nodef1, nodef2) ∈ (frontier1 × frontier2)
        let (mut frontier1, mut frontier2) = try_join!(
            NodeFrontier::new_from_single_node(&ctx, changeset_fetcher.clone(), node1),
            NodeFrontier::new_from_single_node(&ctx, changeset_fetcher.clone(), node2),
        )?;

        // Invariant: generation of lowest common ancestor is always <= gen
        let mut gen = min(
            frontier1
                .max_gen()
                .ok_or(ErrorKind::ProgrammingError("frontier can't be empty"))?,
            frontier2
                .max_gen()
                .ok_or(ErrorKind::ProgrammingError("frontier can't be empty"))?,
        )
        .add(1);
        let mut step = 1;

        let mut ca_gen: Option<Generation> = None; // common ancestor's generation
        loop {
            // We start from advancing both frontiers up to generation=gen-step.
            let candidate_gen = gen.checked_sub(step).unwrap_or(FIRST_GENERATION);
            if candidate_gen == gen {
                // We didn't advance the generation - let's throw.
                Err(ErrorKind::ProgrammingError(
                    "impossible state during LCA computation",
                ))?
            }
            let (candidate_frontier1, candidate_frontier2) = self
                .process_frontiers(
                    &ctx,
                    &changeset_fetcher,
                    &frontier1,
                    &frontier2,
                    candidate_gen,
                )
                .await?;
            let intersection = candidate_frontier1.intersection(&candidate_frontier2);
            if intersection.is_empty() {
                // Intersection is empty so we need to dig deeper. It's safe to advance the
                // frontiers as there is no common node with generation higher than candidate_gen.
                gen = candidate_gen;
                frontier1 = candidate_frontier1;
                frontier2 = candidate_frontier2;
                if self.has_any_skip_edges(&frontier1) && self.has_any_skip_edges(&frontier2) {
                    step = skip_step_size;
                } else {
                    // If there are no skip edges in both frontiers we don't even bother with
                    // skipping further ahead.
                    step = 1;
                }
            } else {
                // Intersection is non-empty so we found a common ancestor.
                ca_gen = Some(candidate_gen);
                break;
            }
            debug_assert!(frontier1.max_gen() == frontier2.max_gen());
            if frontier1
                .max_gen()
                .ok_or(ErrorKind::ProgrammingError("frontier can't be empty"))?
                == FIRST_GENERATION
            {
                break;
            }
        }

        // In case of negative result we can return early.
        let ca_gen = match ca_gen {
            Some(ca_gen) => ca_gen,
            None => {
                return Ok(vec![]);
            }
        };

        // STAGE 2: We know that lca has generation between `gen` and `ca_gen`. Let's find it.
        //
        // NOTE: Right now our skiplists allow us to only skip by 512 commits so the only way to reach
        // closer than that is going one-by-one which is what we're doing here. In the future once we'll
        // get more flexibility arround skips this could be changed to a binary search.
        let mut gen = gen;
        while gen >= ca_gen {
            let (candidate_frontier1, candidate_frontier2) = self
                .process_frontiers(&ctx, &changeset_fetcher, &frontier1, &frontier2, gen)
                .await?;
            let mut intersection = candidate_frontier1.intersection(&candidate_frontier2);
            if let Some((_, lca)) = intersection.remove_max_gen() {
                let mut lca: Vec<_> = lca.into_iter().collect();
                lca.sort();
                return Ok(lca);
            } else {
                frontier1 = candidate_frontier1;
                frontier2 = candidate_frontier2;
                gen = gen.checked_sub(1).ok_or({
                    ErrorKind::ProgrammingError("impossible state during LCA computation")
                })?;
            }
        }
        Err(ErrorKind::ProgrammingError("impossible state during LCA computation").into())
    }

    /// Find all merge commits on the path between two nodes.
    /// where there might be more than one such ancestor, this function is guaranteed to
    /// return all the common ancestors with highest generation number.
    ///
    /// Works by first doing a skiplist-based walk from descendant to ancestor's generation
    /// and then backtracking to find the only visited merge nodes on the path to ancestor.
    pub async fn find_merges_between(
        &self,
        ctx: &CoreContext,
        changeset_fetcher: &ArcChangesetFetcher,
        ancestor: ChangesetId,
        descendant: ChangesetId,
    ) -> Result<Vec<ChangesetId>, Error> {
        let ancestor_gen = fetch_generation(ctx, changeset_fetcher, ancestor).await?;
        let node_frontier =
            NodeFrontier::new_from_single_node(ctx, changeset_fetcher.clone(), descendant).await?;
        let mut trace = SkiplistTraversalTrace::new();

        let node_frontier = process_frontier(
            ctx,
            changeset_fetcher,
            &self.skip_list_edges.load(),
            node_frontier,
            ancestor_gen,
            &Some(&mut trace),
        )
        .await?;
        if match node_frontier.get_all_changesets_for_gen_num(ancestor_gen) {
            Some(cs_ids) => !cs_ids.contains(&ancestor),
            None => true,
        } {
            return Err(ErrorKind::ProgrammingError(
                "ancestor arg is not really an ancestor of descendant arg",
            )
            .into());
        }

        let mut stack = vec![ancestor];
        let mut merges = vec![];

        // DFS walk over the skiplist travesal trace.
        while let Some(cs_id) = stack.pop() {
            if let Some((_key, descendant_entries)) = trace.inner().remove(&cs_id) {
                for (descendant_cs_id, is_merge) in descendant_entries {
                    stack.push(descendant_cs_id);
                    if is_merge {
                        merges.push(descendant_cs_id);
                    }
                }
            }
        }
        Ok(merges)
    }
}

#[cfg(test)]
mod test {
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    use async_trait::async_trait;
    use blobrepo::BlobRepo;
    use bookmarks::BookmarkName;
    use cloned::cloned;
    use context::CoreContext;
    use dashmap::DashMap;
    use fbinit::FacebookInit;
    use futures::compat::Future01CompatExt;
    use futures::stream::iter;
    use futures::stream::StreamExt;
    use futures::stream::TryStreamExt;
    use futures_ext_compat::BoxFuture;
    use futures_ext_compat::FutureExt as FBFutureExt;
    use futures_old::future::join_all;
    use futures_old::future::Future;
    use futures_old::stream::Stream;
    use futures_util::future::FutureExt;
    use futures_util::future::TryFutureExt;
    use revset::AncestorsNodeStream;
    use std::collections::HashSet;

    use super::*;
    use fixtures::BranchEven;
    use fixtures::BranchUneven;
    use fixtures::BranchWide;
    use fixtures::Linear;
    use fixtures::MergeEven;
    use fixtures::MergeUneven;
    use fixtures::TestRepoFixture;
    use fixtures::UnsharedMergeEven;
    use test_helpers::string_to_bonsai;
    use test_helpers::test_branch_wide_reachability;
    use test_helpers::test_linear_reachability;
    use test_helpers::test_merge_uneven_reachability;

    #[tokio::test]
    async fn simple_init() {
        let sli = SkiplistIndex::new();
        assert_eq!(sli.skip_edge_count(), DEFAULT_EDGE_COUNT);

        let sli_with_20 = SkiplistIndex::with_skip_edge_count(20);
        assert_eq!(sli_with_20.skip_edge_count(), 20);
    }

    #[test]
    fn arc_chash_is_sync_and_send() {
        fn is_sync<T: Sync>() {}
        fn is_send<T: Send>() {}

        is_sync::<Arc<DashMap<ChangesetId, SkiplistNodeType>>>();
        is_send::<Arc<DashMap<ChangesetId, SkiplistNodeType>>>();
    }

    #[fbinit::test]
    async fn test_add_node(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(Linear::getrepo(fb).await);
        let sli = SkiplistIndex::new();
        let master_node =
            string_to_bonsai(&ctx, &repo, "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157").await;
        sli.add_node(&ctx, &repo.get_changeset_fetcher(), master_node, 100)
            .await
            .unwrap();
        let ordered_hashes = vec![
            string_to_bonsai(&ctx, &repo, "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157").await,
            string_to_bonsai(&ctx, &repo, "0ed509bf086fadcb8a8a5384dc3b550729b0fc17").await,
            string_to_bonsai(&ctx, &repo, "eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b").await,
            string_to_bonsai(&ctx, &repo, "cb15ca4a43a59acff5388cea9648c162afde8372").await,
            string_to_bonsai(&ctx, &repo, "d0a361e9022d226ae52f689667bd7d212a19cfe0").await,
            string_to_bonsai(&ctx, &repo, "607314ef579bd2407752361ba1b0c1729d08b281").await,
            string_to_bonsai(&ctx, &repo, "3e0e761030db6e479a7fb58b12881883f9f8c63f").await,
            string_to_bonsai(&ctx, &repo, "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").await,
        ];
        assert_eq!(sli.indexed_node_count(), ordered_hashes.len());
        for node in ordered_hashes.into_iter() {
            assert!(sli.is_node_indexed(node));
        }
    }

    #[fbinit::test]
    async fn test_skip_edges_reach_end_in_linear(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(Linear::getrepo(fb).await);
        let sli = SkiplistIndex::new();
        let master_node =
            string_to_bonsai(&ctx, &repo, "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157").await;
        sli.add_node(&ctx, &repo.get_changeset_fetcher(), master_node, 100)
            .await
            .unwrap();
        let ordered_hashes = vec![
            string_to_bonsai(&ctx, &repo, "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157").await,
            string_to_bonsai(&ctx, &repo, "0ed509bf086fadcb8a8a5384dc3b550729b0fc17").await,
            string_to_bonsai(&ctx, &repo, "eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b").await,
            string_to_bonsai(&ctx, &repo, "cb15ca4a43a59acff5388cea9648c162afde8372").await,
            string_to_bonsai(&ctx, &repo, "d0a361e9022d226ae52f689667bd7d212a19cfe0").await,
            string_to_bonsai(&ctx, &repo, "607314ef579bd2407752361ba1b0c1729d08b281").await,
            string_to_bonsai(&ctx, &repo, "3e0e761030db6e479a7fb58b12881883f9f8c63f").await,
            string_to_bonsai(&ctx, &repo, "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").await,
        ];
        assert_eq!(sli.indexed_node_count(), ordered_hashes.len());
        for node in ordered_hashes.into_iter() {
            assert!(sli.is_node_indexed(node));
            if node
                != string_to_bonsai(&ctx, &repo, "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").await
            {
                let skip_edges: Vec<_> = sli
                    .get_skip_edges(node)
                    .unwrap()
                    .into_iter()
                    .map(|(node, _)| node)
                    .collect();
                assert!(
                    skip_edges.contains(
                        &string_to_bonsai(&ctx, &repo, "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")
                            .await
                    )
                );
            }
        }
    }

    #[fbinit::test]
    async fn test_skip_edges_progress_powers_of_2(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(Linear::getrepo(fb).await);
        let sli = SkiplistIndex::new();
        let master_node =
            string_to_bonsai(&ctx, &repo, "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157").await;
        sli.add_node(&ctx, &repo.get_changeset_fetcher(), master_node, 100)
            .await
            .unwrap();
        // hashes in order from newest to oldest are:
        // a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157
        // 0ed509bf086fadcb8a8a5384dc3b550729b0fc17
        // eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b
        // cb15ca4a43a59acff5388cea9648c162afde8372
        // d0a361e9022d226ae52f689667bd7d212a19cfe0
        // 607314ef579bd2407752361ba1b0c1729d08b281
        // 3e0e761030db6e479a7fb58b12881883f9f8c63f
        // 2d7d4ba9ce0a6ffd222de7785b249ead9c51c536

        // the skip edges for the master node should be:
        // - 0ed5
        // - eed3
        // - d0a3
        // - 2d7d

        let skip_edges: Vec<_> = sli
            .get_skip_edges(master_node)
            .unwrap()
            .into_iter()
            .map(|(node, _)| node)
            .collect();
        let expected_hashes = vec![
            string_to_bonsai(&ctx, &repo, "0ed509bf086fadcb8a8a5384dc3b550729b0fc17").await,
            string_to_bonsai(&ctx, &repo, "eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b").await,
            string_to_bonsai(&ctx, &repo, "d0a361e9022d226ae52f689667bd7d212a19cfe0").await,
            string_to_bonsai(&ctx, &repo, "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").await,
        ];
        assert_eq!(skip_edges, expected_hashes);
    }

    #[fbinit::test]
    async fn test_skip_edges_reach_end_in_merge(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(MergeUneven::getrepo(fb).await);
        let root_node =
            string_to_bonsai(&ctx, &repo, "15c40d0abc36d47fb51c8eaec51ac7aad31f669c").await;

        // order is oldest to newest
        let branch_1 = vec![
            string_to_bonsai(&ctx, &repo, "3cda5c78aa35f0f5b09780d971197b51cad4613a").await,
            string_to_bonsai(&ctx, &repo, "1d8a907f7b4bf50c6a09c16361e2205047ecc5e5").await,
            string_to_bonsai(&ctx, &repo, "16839021e338500b3cf7c9b871c8a07351697d68").await,
        ];

        // order is oldest to newest
        let branch_2 = vec![
            string_to_bonsai(&ctx, &repo, "d7542c9db7f4c77dab4b315edd328edf1514952f").await,
            string_to_bonsai(&ctx, &repo, "b65231269f651cfe784fd1d97ef02a049a37b8a0").await,
            string_to_bonsai(&ctx, &repo, "4f7f3fd428bec1a48f9314414b063c706d9c1aed").await,
            string_to_bonsai(&ctx, &repo, "795b8133cf375f6d68d27c6c23db24cd5d0cd00f").await,
            string_to_bonsai(&ctx, &repo, "bc7b4d0f858c19e2474b03e442b8495fd7aeef33").await,
            string_to_bonsai(&ctx, &repo, "fc2cef43395ff3a7b28159007f63d6529d2f41ca").await,
            string_to_bonsai(&ctx, &repo, "5d43888a3c972fe68c224f93d41b30e9f888df7c").await,
            string_to_bonsai(&ctx, &repo, "264f01429683b3dd8042cb3979e8bf37007118bc").await,
        ];

        let merge_node =
            string_to_bonsai(&ctx, &repo, "d35b1875cdd1ed2c687e86f1604b9d7e989450cb").await;
        let sli = SkiplistIndex::new();
        sli.add_node(&ctx, &repo.get_changeset_fetcher(), merge_node, 100)
            .await
            .unwrap();
        for node in branch_1.into_iter() {
            let skip_edges: Vec<_> = sli
                .get_skip_edges(node)
                .unwrap()
                .into_iter()
                .map(|(node, _)| node)
                .collect();
            assert!(skip_edges.contains(&root_node));
        }
        for node in branch_2.into_iter() {
            let skip_edges: Vec<_> = sli
                .get_skip_edges(node)
                .unwrap()
                .into_iter()
                .map(|(node, _)| node)
                .collect();
            assert!(skip_edges.contains(&root_node));
        }
        // the merge node is indexed but has parent edges, not skip edges
        assert!(sli.is_node_indexed(merge_node));
        assert_eq!(sli.get_skip_edges(merge_node), None);
    }

    #[fbinit::test]
    async fn test_partial_index_in_merge(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(MergeUneven::getrepo(fb).await);
        let root_node =
            string_to_bonsai(&ctx, &repo, "15c40d0abc36d47fb51c8eaec51ac7aad31f669c").await;

        // order is oldest to newest
        let branch_1 = vec![
            string_to_bonsai(&ctx, &repo, "3cda5c78aa35f0f5b09780d971197b51cad4613a").await,
            string_to_bonsai(&ctx, &repo, "1d8a907f7b4bf50c6a09c16361e2205047ecc5e5").await,
            string_to_bonsai(&ctx, &repo, "16839021e338500b3cf7c9b871c8a07351697d68").await,
        ];

        let branch_1_head =
            string_to_bonsai(&ctx, &repo, "16839021e338500b3cf7c9b871c8a07351697d68").await;

        // order is oldest to newest
        let branch_2 = vec![
            string_to_bonsai(&ctx, &repo, "d7542c9db7f4c77dab4b315edd328edf1514952f").await,
            string_to_bonsai(&ctx, &repo, "b65231269f651cfe784fd1d97ef02a049a37b8a0").await,
            string_to_bonsai(&ctx, &repo, "4f7f3fd428bec1a48f9314414b063c706d9c1aed").await,
            string_to_bonsai(&ctx, &repo, "795b8133cf375f6d68d27c6c23db24cd5d0cd00f").await,
            string_to_bonsai(&ctx, &repo, "bc7b4d0f858c19e2474b03e442b8495fd7aeef33").await,
            string_to_bonsai(&ctx, &repo, "fc2cef43395ff3a7b28159007f63d6529d2f41ca").await,
            string_to_bonsai(&ctx, &repo, "5d43888a3c972fe68c224f93d41b30e9f888df7c").await,
            string_to_bonsai(&ctx, &repo, "264f01429683b3dd8042cb3979e8bf37007118bc").await,
        ];
        let branch_2_head =
            string_to_bonsai(&ctx, &repo, "264f01429683b3dd8042cb3979e8bf37007118bc").await;

        let _merge_node =
            string_to_bonsai(&ctx, &repo, "d35b1875cdd1ed2c687e86f1604b9d7e989450cb").await;
        let sli = SkiplistIndex::new();

        // index just one branch first
        sli.add_node(&ctx, &repo.get_changeset_fetcher(), branch_1_head, 100)
            .await
            .unwrap();
        for node in branch_1.into_iter() {
            let skip_edges: Vec<_> = sli
                .get_skip_edges(node)
                .unwrap()
                .into_iter()
                .map(|(node, _)| node)
                .collect();
            assert!(skip_edges.contains(&root_node));
        }
        for node in branch_2.clone().into_iter() {
            assert!(!sli.is_node_indexed(node));
        }
        // index second branch
        sli.add_node(&ctx, &repo.get_changeset_fetcher(), branch_2_head, 100)
            .await
            .unwrap();
        for node in branch_2.into_iter() {
            let skip_edges: Vec<_> = sli
                .get_skip_edges(node)
                .unwrap()
                .into_iter()
                .map(|(node, _)| node)
                .collect();
            assert!(skip_edges.contains(&root_node));
        }
    }

    #[fbinit::test]
    async fn test_simul_index_on_wide_branch(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        // this repo has no merges but many branches
        let repo = Arc::new(BranchWide::getrepo(fb).await);
        let root_node =
            string_to_bonsai(&ctx, &repo, "ecba698fee57eeeef88ac3dcc3b623ede4af47bd").await;

        let b1 = string_to_bonsai(&ctx, &repo, "9e8521affb7f9d10e9551a99c526e69909042b20").await;
        let b2 = string_to_bonsai(&ctx, &repo, "4685e9e62e4885d477ead6964a7600c750e39b03").await;
        let b1_1 = string_to_bonsai(&ctx, &repo, "b6a8169454af58b4b72b3665f9aa0d25529755ff").await;
        let b1_2 = string_to_bonsai(&ctx, &repo, "c27ef5b7f15e9930e5b93b1f32cc2108a2aabe12").await;
        let b2_1 = string_to_bonsai(&ctx, &repo, "04decbb0d1a65789728250ddea2fe8d00248e01c").await;
        let b2_2 = string_to_bonsai(&ctx, &repo, "49f53ab171171b3180e125b918bd1cf0af7e5449").await;

        let sli = SkiplistIndex::new();
        let changeset_fetcher = repo.get_changeset_fetcher();
        iter(vec![b1_1, b1_2, b2_1, b2_2].into_iter())
            .map(|branch_tip| Ok(sli.add_node(&ctx, &changeset_fetcher, branch_tip, 100)))
            .try_buffer_unordered(4)
            .try_for_each(|_| async { Ok(()) })
            .await
            .unwrap();
        assert!(sli.is_node_indexed(root_node));
        assert!(sli.is_node_indexed(b1));
        assert!(sli.is_node_indexed(b2));

        for node in vec![b1, b2, b1_1, b1_2, b2_1, b2_2].into_iter() {
            let skip_edges: Vec<_> = sli
                .get_skip_edges(node)
                .unwrap()
                .into_iter()
                .map(|(node, _)| node)
                .collect();
            assert!(skip_edges.contains(&root_node));
        }
    }

    #[fbinit::test]
    async fn linear_reachability(fb: FacebookInit) {
        let sli_constructor = SkiplistIndex::new;
        test_linear_reachability(fb, sli_constructor).await;
    }

    #[fbinit::test]
    async fn merge_uneven_reachability(fb: FacebookInit) {
        let sli_constructor = SkiplistIndex::new;
        test_merge_uneven_reachability(fb, sli_constructor).await;
    }

    #[fbinit::test]
    async fn branch_wide_reachability(fb: FacebookInit) {
        let sli_constructor = SkiplistIndex::new;
        test_branch_wide_reachability(fb, sli_constructor).await;
    }

    struct CountingChangesetFetcher {
        pub get_parents_count: Arc<AtomicUsize>,
        pub get_gen_number_count: Arc<AtomicUsize>,
        cs_fetcher: ArcChangesetFetcher,
    }

    impl CountingChangesetFetcher {
        fn new(
            cs_fetcher: ArcChangesetFetcher,
            get_parents_count: Arc<AtomicUsize>,
            get_gen_number_count: Arc<AtomicUsize>,
        ) -> Self {
            Self {
                get_parents_count,
                get_gen_number_count,
                cs_fetcher,
            }
        }
    }

    #[async_trait]
    impl ChangesetFetcher for CountingChangesetFetcher {
        async fn get_generation_number(
            &self,
            ctx: CoreContext,
            cs_id: ChangesetId,
        ) -> Result<Generation, Error> {
            self.get_gen_number_count.fetch_add(1, Ordering::Relaxed);
            self.cs_fetcher.get_generation_number(ctx, cs_id).await
        }

        async fn get_parents(
            &self,
            ctx: CoreContext,
            cs_id: ChangesetId,
        ) -> Result<Vec<ChangesetId>, Error> {
            self.get_parents_count.fetch_add(1, Ordering::Relaxed);
            self.cs_fetcher.get_parents(ctx, cs_id).await
        }
    }

    async fn query_reachability_hint_on_self_is_true(
        ctx: CoreContext,
        repo: BlobRepo,
        sli: SkiplistIndex,
    ) {
        let mut ordered_hashes_oldest_to_newest = vec![
            string_to_bonsai(&ctx, &repo, "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157").await,
            string_to_bonsai(&ctx, &repo, "0ed509bf086fadcb8a8a5384dc3b550729b0fc17").await,
            string_to_bonsai(&ctx, &repo, "eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b").await,
            string_to_bonsai(&ctx, &repo, "cb15ca4a43a59acff5388cea9648c162afde8372").await,
            string_to_bonsai(&ctx, &repo, "d0a361e9022d226ae52f689667bd7d212a19cfe0").await,
            string_to_bonsai(&ctx, &repo, "607314ef579bd2407752361ba1b0c1729d08b281").await,
            string_to_bonsai(&ctx, &repo, "3e0e761030db6e479a7fb58b12881883f9f8c63f").await,
            string_to_bonsai(&ctx, &repo, "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").await,
        ];
        ordered_hashes_oldest_to_newest.reverse();
        // indexing doesn't even take place if the query can conclude true or false right away

        for (_, node) in ordered_hashes_oldest_to_newest.into_iter().enumerate() {
            assert!(
                sli.query_reachability(&ctx, &repo.get_changeset_fetcher(), node, node)
                    .await
                    .unwrap()
            );
        }
    }

    macro_rules! skiplist_test {
        ($test_name:ident, $repo:ident) => {
            mod $test_name {
                use super::*;
                #[fbinit::test]
                async fn no_index(fb: FacebookInit) {
                    let ctx = CoreContext::test_mock(fb);
                    let repo = $repo::getrepo(fb).await;
                    let sli = SkiplistIndex::new();
                    $test_name(ctx, repo, sli).await
                }

                #[fbinit::test]
                async fn all_indexed(fb: FacebookInit) {
                    let ctx = CoreContext::test_mock(fb);
                    let repo = $repo::getrepo(fb).await;
                    let sli = SkiplistIndex::new();
                    {
                        let heads = repo
                            .get_bonsai_heads_maybe_stale(ctx.clone())
                            .try_collect::<Vec<_>>()
                            .await
                            .unwrap();
                        for head in heads {
                            sli.add_node(&ctx, &repo.get_changeset_fetcher(), head, 100)
                                .await
                                .unwrap();
                        }
                    }
                    $test_name(ctx, repo, sli).await;
                }
            }
        };
    }

    async fn query_reachability_to_higher_gen_is_false(
        ctx: CoreContext,
        repo: BlobRepo,
        sli: SkiplistIndex,
    ) {
        let mut ordered_hashes_oldest_to_newest = vec![
            string_to_bonsai(&ctx, &repo, "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157").await,
            string_to_bonsai(&ctx, &repo, "0ed509bf086fadcb8a8a5384dc3b550729b0fc17").await,
            string_to_bonsai(&ctx, &repo, "eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b").await,
            string_to_bonsai(&ctx, &repo, "cb15ca4a43a59acff5388cea9648c162afde8372").await,
            string_to_bonsai(&ctx, &repo, "d0a361e9022d226ae52f689667bd7d212a19cfe0").await,
            string_to_bonsai(&ctx, &repo, "607314ef579bd2407752361ba1b0c1729d08b281").await,
            string_to_bonsai(&ctx, &repo, "3e0e761030db6e479a7fb58b12881883f9f8c63f").await,
            string_to_bonsai(&ctx, &repo, "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").await,
        ];
        ordered_hashes_oldest_to_newest.reverse();

        // indexing doesn't even take place if the query can conclude true or false right away
        for i in 0..ordered_hashes_oldest_to_newest.len() {
            let src_node = ordered_hashes_oldest_to_newest.get(i).unwrap();
            for j in i + 1..ordered_hashes_oldest_to_newest.len() {
                let dst_node = ordered_hashes_oldest_to_newest.get(j).unwrap();
                sli.query_reachability(&ctx, &repo.get_changeset_fetcher(), *src_node, *dst_node)
                    .await
                    .unwrap();
            }
        }
    }

    #[fbinit::test]
    async fn test_query_reachability_from_unindexed_node(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(Linear::getrepo(fb).await);
        let sli = SkiplistIndex::new();
        let get_parents_count = Arc::new(AtomicUsize::new(0));
        let get_gen_number_count = Arc::new(AtomicUsize::new(0));
        let cs_fetcher: ArcChangesetFetcher = Arc::new(CountingChangesetFetcher::new(
            repo.get_changeset_fetcher(),
            get_parents_count.clone(),
            get_gen_number_count,
        ));

        let src_node =
            string_to_bonsai(&ctx, &repo, "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157").await;
        let dst_node =
            string_to_bonsai(&ctx, &repo, "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").await;
        sli.query_reachability(&ctx, &cs_fetcher, src_node, dst_node)
            .await
            .unwrap();
        let ordered_hashes = vec![
            string_to_bonsai(&ctx, &repo, "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157").await,
            string_to_bonsai(&ctx, &repo, "0ed509bf086fadcb8a8a5384dc3b550729b0fc17").await,
            string_to_bonsai(&ctx, &repo, "eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b").await,
            string_to_bonsai(&ctx, &repo, "cb15ca4a43a59acff5388cea9648c162afde8372").await,
            string_to_bonsai(&ctx, &repo, "d0a361e9022d226ae52f689667bd7d212a19cfe0").await,
            string_to_bonsai(&ctx, &repo, "607314ef579bd2407752361ba1b0c1729d08b281").await,
            string_to_bonsai(&ctx, &repo, "3e0e761030db6e479a7fb58b12881883f9f8c63f").await,
            string_to_bonsai(&ctx, &repo, "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").await,
        ];
        // Nothing is indexed by default
        assert_eq!(sli.indexed_node_count(), 0);
        for node in ordered_hashes.iter() {
            assert!(!sli.is_node_indexed(*node));
        }

        let parents_count_before_indexing = get_parents_count.load(Ordering::Relaxed);
        assert!(parents_count_before_indexing > 0);

        // Index
        sli.add_node(&ctx, &repo.get_changeset_fetcher(), src_node, 10)
            .await
            .unwrap();
        assert_eq!(sli.indexed_node_count(), ordered_hashes.len());
        for node in ordered_hashes.into_iter() {
            assert!(sli.is_node_indexed(node));
        }

        // Make sure that we don't use changeset fetcher anymore, because everything is
        // indexed
        let f = sli.query_reachability(&ctx, &cs_fetcher, src_node, dst_node);

        assert!(f.await.unwrap());

        assert_eq!(
            parents_count_before_indexing,
            get_parents_count.load(Ordering::Relaxed)
        );
    }

    async fn query_from_indexed_merge_node(ctx: CoreContext, repo: BlobRepo, sli: SkiplistIndex) {
        let merge_node =
            string_to_bonsai(&ctx, &repo, "d592490c4386cdb3373dd93af04d563de199b2fb").await;
        let commit_after_merge =
            string_to_bonsai(&ctx, &repo, "7fe9947f101acb4acf7d945e69f0d6ce76a81113").await;
        let cs_fetcher = repo.get_changeset_fetcher();
        // Indexing starting from a merge node
        sli.add_node(&ctx, &repo.get_changeset_fetcher(), commit_after_merge, 10)
            .await
            .unwrap();
        let f = sli.query_reachability(&ctx, &cs_fetcher, commit_after_merge, merge_node);
        assert!(f.await.unwrap());

        // perform a query from the merge to the start of branch 1
        let dst_node =
            string_to_bonsai(&ctx, &repo, "1700524113b1a3b1806560341009684b4378660b").await;
        // performing this query should index all the nodes inbetween
        let f = sli.query_reachability(&ctx, &cs_fetcher, merge_node, dst_node);
        assert!(f.await.unwrap());

        // perform a query from the merge to the start of branch 2
        let dst_node =
            string_to_bonsai(&ctx, &repo, "1700524113b1a3b1806560341009684b4378660b").await;
        let f = sli.query_reachability(&ctx, &cs_fetcher, merge_node, dst_node);
        assert!(f.await.unwrap());
    }

    fn advance_node_forward(
        ctx: CoreContext,
        changeset_fetcher: ArcChangesetFetcher,
        skip_list_edges: Arc<SkiplistEdgeMapping>,
        (node, gen): (ChangesetId, Generation),
        max_gen: Generation,
    ) -> BoxFuture<NodeFrontier, Error> {
        let initial_frontier = hashmap! {gen => hashset!{node}};
        let initial_frontier = NodeFrontier::new(initial_frontier);
        async move {
            process_frontier(
                &ctx,
                &changeset_fetcher,
                &skip_list_edges,
                initial_frontier,
                max_gen,
                &None,
            )
            .await
        }
        .boxed()
        .compat()
        .boxify()
    }

    async fn advance_node_linear(ctx: CoreContext, repo: BlobRepo, sli: SkiplistIndex) {
        let mut ordered_hashes_oldest_to_newest = vec![
            string_to_bonsai(&ctx, &repo, "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157").await,
            string_to_bonsai(&ctx, &repo, "0ed509bf086fadcb8a8a5384dc3b550729b0fc17").await,
            string_to_bonsai(&ctx, &repo, "eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b").await,
            string_to_bonsai(&ctx, &repo, "cb15ca4a43a59acff5388cea9648c162afde8372").await,
            string_to_bonsai(&ctx, &repo, "d0a361e9022d226ae52f689667bd7d212a19cfe0").await,
            string_to_bonsai(&ctx, &repo, "607314ef579bd2407752361ba1b0c1729d08b281").await,
            string_to_bonsai(&ctx, &repo, "3e0e761030db6e479a7fb58b12881883f9f8c63f").await,
            string_to_bonsai(&ctx, &repo, "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").await,
        ];
        ordered_hashes_oldest_to_newest.reverse();

        for (gen, node) in ordered_hashes_oldest_to_newest
            .clone()
            .into_iter()
            .enumerate()
        {
            // advancing any node to any earlier generation
            // will get a unique node in the linear segment
            for gen_earlier in 0..gen {
                let earlier_node = ordered_hashes_oldest_to_newest.get(gen_earlier).unwrap();
                let mut expected_frontier_map = HashMap::new();
                expected_frontier_map.insert(
                    Generation::new(gen_earlier as u64 + 1),
                    vec![earlier_node].into_iter().cloned().collect(),
                );
                let f = advance_node_forward(
                    ctx.clone(),
                    repo.get_changeset_fetcher(),
                    sli.skip_list_edges.load_full(),
                    (node, Generation::new(gen as u64 + 1)),
                    Generation::new(gen_earlier as u64 + 1),
                );
                assert_eq!(
                    f.compat().await.unwrap(),
                    NodeFrontier::new(expected_frontier_map),
                );
            }
        }
        for (gen, node) in ordered_hashes_oldest_to_newest
            .clone()
            .into_iter()
            .enumerate()
        {
            // attempting to advance to a larger gen gives a frontier with the start node
            // this fits with the definition of what advance_node should do
            for gen_later in gen + 1..ordered_hashes_oldest_to_newest.len() {
                let mut expected_frontier_map = HashMap::new();
                expected_frontier_map.insert(
                    Generation::new(gen as u64 + 1),
                    vec![node].into_iter().collect(),
                );
                let f = advance_node_forward(
                    ctx.clone(),
                    repo.get_changeset_fetcher(),
                    sli.skip_list_edges.load_full(),
                    (node, Generation::new(gen as u64 + 1)),
                    Generation::new(gen_later as u64 + 1),
                );
                assert_eq!(
                    f.compat().await.unwrap(),
                    NodeFrontier::new(expected_frontier_map)
                );
            }
        }
    }

    async fn advance_node_uneven_merge(ctx: CoreContext, repo: BlobRepo, sli: SkiplistIndex) {
        let root_node =
            string_to_bonsai(&ctx, &repo, "15c40d0abc36d47fb51c8eaec51ac7aad31f669c").await;

        // order is oldest to newest
        let branch_1 = vec![
            string_to_bonsai(&ctx, &repo, "3cda5c78aa35f0f5b09780d971197b51cad4613a").await,
            string_to_bonsai(&ctx, &repo, "1d8a907f7b4bf50c6a09c16361e2205047ecc5e5").await,
            string_to_bonsai(&ctx, &repo, "16839021e338500b3cf7c9b871c8a07351697d68").await,
        ];

        // order is oldest to newest
        let branch_2 = vec![
            string_to_bonsai(&ctx, &repo, "d7542c9db7f4c77dab4b315edd328edf1514952f").await,
            string_to_bonsai(&ctx, &repo, "b65231269f651cfe784fd1d97ef02a049a37b8a0").await,
            string_to_bonsai(&ctx, &repo, "4f7f3fd428bec1a48f9314414b063c706d9c1aed").await,
            string_to_bonsai(&ctx, &repo, "795b8133cf375f6d68d27c6c23db24cd5d0cd00f").await,
            string_to_bonsai(&ctx, &repo, "bc7b4d0f858c19e2474b03e442b8495fd7aeef33").await,
            string_to_bonsai(&ctx, &repo, "fc2cef43395ff3a7b28159007f63d6529d2f41ca").await,
            string_to_bonsai(&ctx, &repo, "5d43888a3c972fe68c224f93d41b30e9f888df7c").await,
            string_to_bonsai(&ctx, &repo, "264f01429683b3dd8042cb3979e8bf37007118bc").await,
        ];
        let merge_node =
            string_to_bonsai(&ctx, &repo, "d35b1875cdd1ed2c687e86f1604b9d7e989450cb").await;

        // This test tries to advance the merge node forward.
        // It should test the part of `advance_node_forward` for when the node doesn't
        // have skip list edges.

        // Generation 2 to 4
        // Will have nodes in both branches at this generation
        for gen in 0..branch_1.len() {
            let mut expected_frontier_map = HashMap::new();
            let frontier_generation = Generation::new(gen as u64 + 2);
            expected_frontier_map.insert(
                frontier_generation,
                vec![
                    branch_1.get(gen).unwrap().clone(),
                    branch_2.get(gen).unwrap().clone(),
                ]
                .into_iter()
                .collect(),
            );
            let f = advance_node_forward(
                ctx.clone(),
                repo.get_changeset_fetcher(),
                sli.skip_list_edges.load_full(),
                (merge_node, Generation::new(10)),
                frontier_generation,
            );
            assert_eq!(
                f.compat().await.unwrap(),
                NodeFrontier::new(expected_frontier_map),
            );
        }

        // Generation 4 to 9
        // Will have nodes from both branches
        // But the branch 1 node will be stuck at generation 3
        let branch_1_head = branch_1.clone().into_iter().last().unwrap();
        let branch_1_head_gen = Generation::new(branch_1.len() as u64 + 1);
        for gen in branch_1.len()..branch_2.len() {
            let mut expected_frontier_map = HashMap::new();
            let frontier_generation = Generation::new(gen as u64 + 2);
            assert!(branch_1_head_gen != frontier_generation);
            expected_frontier_map.insert(
                frontier_generation,
                vec![branch_2.get(gen).unwrap().clone()]
                    .into_iter()
                    .collect(),
            );
            expected_frontier_map.insert(
                branch_1_head_gen,
                vec![branch_1_head.clone()].into_iter().collect(),
            );
            let f = advance_node_forward(
                ctx.clone(),
                repo.get_changeset_fetcher(),
                sli.skip_list_edges.load_full(),
                (merge_node, Generation::new(10)),
                frontier_generation,
            );
            assert_eq!(
                f.compat().await.unwrap(),
                NodeFrontier::new(expected_frontier_map)
            );
        }

        // Generation 1
        let mut expected_root_frontier_map = HashMap::new();
        expected_root_frontier_map
            .insert(Generation::new(1), vec![root_node].into_iter().collect());
        let f = advance_node_forward(
            ctx,
            repo.get_changeset_fetcher(),
            sli.skip_list_edges.load_full(),
            (merge_node, Generation::new(10)),
            Generation::new(1),
        );
        assert_eq!(
            f.compat().await.unwrap(),
            NodeFrontier::new(expected_root_frontier_map),
        );
    }

    async fn advance_node_on_partial_index(ctx: CoreContext, repo: BlobRepo, sli: SkiplistIndex) {
        let root_node =
            string_to_bonsai(&ctx, &repo, "15c40d0abc36d47fb51c8eaec51ac7aad31f669c").await;

        // order is oldest to newest
        let branch_1 = vec![
            string_to_bonsai(&ctx, &repo, "3cda5c78aa35f0f5b09780d971197b51cad4613a").await,
            string_to_bonsai(&ctx, &repo, "1d8a907f7b4bf50c6a09c16361e2205047ecc5e5").await,
            string_to_bonsai(&ctx, &repo, "16839021e338500b3cf7c9b871c8a07351697d68").await,
        ];

        // order is oldest to newest
        let branch_2 = vec![
            string_to_bonsai(&ctx, &repo, "d7542c9db7f4c77dab4b315edd328edf1514952f").await,
            string_to_bonsai(&ctx, &repo, "b65231269f651cfe784fd1d97ef02a049a37b8a0").await,
            string_to_bonsai(&ctx, &repo, "4f7f3fd428bec1a48f9314414b063c706d9c1aed").await,
            string_to_bonsai(&ctx, &repo, "795b8133cf375f6d68d27c6c23db24cd5d0cd00f").await,
            string_to_bonsai(&ctx, &repo, "bc7b4d0f858c19e2474b03e442b8495fd7aeef33").await,
            string_to_bonsai(&ctx, &repo, "fc2cef43395ff3a7b28159007f63d6529d2f41ca").await,
            string_to_bonsai(&ctx, &repo, "5d43888a3c972fe68c224f93d41b30e9f888df7c").await,
            string_to_bonsai(&ctx, &repo, "264f01429683b3dd8042cb3979e8bf37007118bc").await,
        ];

        let merge_node =
            string_to_bonsai(&ctx, &repo, "d35b1875cdd1ed2c687e86f1604b9d7e989450cb").await;

        // This test partially indexes the top few of the graph.
        // Then it does a query that traverses from indexed to unindexed nodes.
        sli.add_node(&ctx, &repo.get_changeset_fetcher(), merge_node, 2)
            .await
            .unwrap();

        // Generation 1
        // This call should index the rest of the graph,
        // but due to the parital index, the skip edges may not jump past
        // where the partial index ended.
        // So we repeat the same tests to check for correctness.
        let mut expected_root_frontier_map = HashMap::new();
        expected_root_frontier_map
            .insert(Generation::new(1), vec![root_node].into_iter().collect());
        let f = advance_node_forward(
            ctx.clone(),
            repo.get_changeset_fetcher(),
            sli.skip_list_edges.load_full(),
            (merge_node, Generation::new(10)),
            Generation::new(1),
        );
        assert_eq!(
            f.compat().await.unwrap(),
            NodeFrontier::new(expected_root_frontier_map),
        );

        // Generation 2 to 4
        // Will have nodes in both branches at this generation
        for gen in 0..branch_1.len() {
            let mut expected_frontier_map = HashMap::new();
            let frontier_generation = Generation::new(gen as u64 + 2);
            expected_frontier_map.insert(
                frontier_generation,
                vec![
                    branch_1.get(gen).unwrap().clone(),
                    branch_2.get(gen).unwrap().clone(),
                ]
                .into_iter()
                .collect(),
            );
            let f = advance_node_forward(
                ctx.clone(),
                repo.get_changeset_fetcher(),
                sli.skip_list_edges.load_full(),
                (merge_node, Generation::new(10)),
                frontier_generation,
            );
            assert_eq!(
                f.compat().await.unwrap(),
                NodeFrontier::new(expected_frontier_map)
            );
        }

        // Generation 4 to 9
        // Will have nodes from both branches
        // But the branch 1 node will be stuck at generation 3
        let branch_1_head = branch_1.clone().into_iter().last().unwrap();
        let branch_1_head_gen = Generation::new(branch_1.len() as u64 + 1);
        for gen in branch_1.len()..branch_2.len() {
            let mut expected_frontier_map = HashMap::new();
            let frontier_generation = Generation::new(gen as u64 + 2);
            assert!(branch_1_head_gen != frontier_generation);
            expected_frontier_map.insert(
                frontier_generation,
                vec![branch_2.get(gen).unwrap().clone()]
                    .into_iter()
                    .collect(),
            );
            expected_frontier_map.insert(
                branch_1_head_gen,
                vec![branch_1_head.clone()].into_iter().collect(),
            );
            let f = advance_node_forward(
                ctx.clone(),
                repo.get_changeset_fetcher(),
                sli.skip_list_edges.load_full(),
                (merge_node, Generation::new(10)),
                frontier_generation,
            );
            assert_eq!(
                f.compat().await.unwrap(),
                NodeFrontier::new(expected_frontier_map)
            );
        }
    }

    async fn simul_node_advance_on_wide_branch(
        ctx: CoreContext,
        repo: BlobRepo,
        sli: SkiplistIndex,
    ) {
        let root_node =
            string_to_bonsai(&ctx, &repo, "ecba698fee57eeeef88ac3dcc3b623ede4af47bd").await;

        let _b1 = string_to_bonsai(&ctx, &repo, "9e8521affb7f9d10e9551a99c526e69909042b20").await;
        let _b2 = string_to_bonsai(&ctx, &repo, "4685e9e62e4885d477ead6964a7600c750e39b03").await;
        let b1_1 = string_to_bonsai(&ctx, &repo, "b6a8169454af58b4b72b3665f9aa0d25529755ff").await;
        let b1_2 = string_to_bonsai(&ctx, &repo, "c27ef5b7f15e9930e5b93b1f32cc2108a2aabe12").await;
        let b2_1 = string_to_bonsai(&ctx, &repo, "04decbb0d1a65789728250ddea2fe8d00248e01c").await;
        let b2_2 = string_to_bonsai(&ctx, &repo, "49f53ab171171b3180e125b918bd1cf0af7e5449").await;

        let advance_to_root_futures =
            vec![b1_1, b1_2, b2_1, b2_2]
                .into_iter()
                .map(move |branch_tip| {
                    advance_node_forward(
                        ctx.clone(),
                        repo.get_changeset_fetcher(),
                        sli.skip_list_edges.load_full(),
                        (branch_tip, Generation::new(3)),
                        Generation::new(1),
                    )
                });
        let advanced_frontiers = join_all(advance_to_root_futures);
        let advanced_frontiers = advanced_frontiers.compat().await.unwrap();
        let mut expected_root_frontier_map = HashMap::new();
        expected_root_frontier_map
            .insert(Generation::new(1), vec![root_node].into_iter().collect());

        let expected_root_frontier = NodeFrontier::new(expected_root_frontier_map);
        for frontier in advanced_frontiers.into_iter() {
            assert_eq!(frontier, expected_root_frontier);
        }
    }

    async fn process_frontier_on_wide_branch(ctx: CoreContext, repo: BlobRepo, sli: SkiplistIndex) {
        let root_node =
            string_to_bonsai(&ctx, &repo, "ecba698fee57eeeef88ac3dcc3b623ede4af47bd").await;

        let b1 = string_to_bonsai(&ctx, &repo, "9e8521affb7f9d10e9551a99c526e69909042b20").await;
        let b2 = string_to_bonsai(&ctx, &repo, "4685e9e62e4885d477ead6964a7600c750e39b03").await;
        let b1_1 = string_to_bonsai(&ctx, &repo, "b6a8169454af58b4b72b3665f9aa0d25529755ff").await;
        let b1_2 = string_to_bonsai(&ctx, &repo, "c27ef5b7f15e9930e5b93b1f32cc2108a2aabe12").await;
        let b2_1 = string_to_bonsai(&ctx, &repo, "04decbb0d1a65789728250ddea2fe8d00248e01c").await;
        let b2_2 = string_to_bonsai(&ctx, &repo, "49f53ab171171b3180e125b918bd1cf0af7e5449").await;

        let mut starting_frontier_map = HashMap::new();
        starting_frontier_map.insert(
            Generation::new(3),
            vec![b1_1, b1_2, b2_1, b2_2].into_iter().collect(),
        );

        let mut expected_gen_2_frontier_map = HashMap::new();
        expected_gen_2_frontier_map.insert(Generation::new(2), vec![b1, b2].into_iter().collect());
        let f = process_frontier(
            &ctx,
            &repo.get_changeset_fetcher(),
            &sli.skip_list_edges.load(),
            NodeFrontier::new(starting_frontier_map.clone()),
            Generation::new(2),
            &None,
        )
        .await;
        assert_eq!(f.unwrap(), NodeFrontier::new(expected_gen_2_frontier_map));

        let mut expected_root_frontier_map = HashMap::new();
        expected_root_frontier_map
            .insert(Generation::new(1), vec![root_node].into_iter().collect());
        let mut trace = SkiplistTraversalTrace::new();
        let f = process_frontier(
            &ctx,
            &repo.get_changeset_fetcher(),
            &sli.skip_list_edges.load(),
            NodeFrontier::new(starting_frontier_map),
            Generation::new(1),
            &Some(&mut trace),
        )
        .await;
        assert_eq!(f.unwrap(), NodeFrontier::new(expected_root_frontier_map));
        let (cs_id, skiplist_node) = trace
            .inner()
            .get(&root_node)
            .unwrap()
            .get(0)
            .unwrap()
            .clone();

        if let Some(_skiplist_edges) = sli.get_skip_edges(cs_id) {
            // When the index is present we see if what's in skiplist for cs_id
            // matches what's in the trace.
            assert_eq!(skiplist_node, false,);
        } else {
            // When the index is empty, we check that parent edge was traversed.
            assert_eq!(skiplist_node, false,);
        }
    }

    async fn test_lca(
        ctx: CoreContext,
        repo: BlobRepo,
        sli: SkiplistIndex,
        b1: &'static str,
        b2: &'static str,
        lca: Option<&'static str>,
    ) {
        let b1 = string_to_bonsai(&ctx, &repo, b1).await;
        let b2 = string_to_bonsai(&ctx, &repo, b2).await;
        let expected = if let Some(lca) = lca {
            Some(string_to_bonsai(&ctx, &repo, lca).await)
        } else {
            None
        };
        let lca = sli
            .lca(ctx, repo.get_changeset_fetcher(), b1.clone(), b2.clone())
            .await
            .unwrap();

        assert_eq!(lca, expected.into_iter().collect::<Vec<_>>());
    }

    async fn test_find_merge(
        ctx: CoreContext,
        repo: BlobRepo,
        sli: SkiplistIndex,
        ancestor: &'static str,
        descendant: &'static str,
        merge_commit: Option<&'static str>,
    ) {
        let ba = string_to_bonsai(&ctx, &repo, ancestor).await;
        let bd = string_to_bonsai(&ctx, &repo, descendant).await;
        let expected = if let Some(merge_commit) = merge_commit {
            Some(string_to_bonsai(&ctx, &repo, merge_commit).await)
        } else {
            None
        };
        let merges = sli
            .find_merges_between(&ctx, &repo.get_changeset_fetcher(), ba.clone(), bd.clone())
            .await
            .unwrap();

        assert_eq!(merges, expected.into_iter().collect::<Vec<_>>());
    }

    async fn test_is_ancestor(ctx: CoreContext, repo: BlobRepo, sli: SkiplistIndex) {
        let f = repo
            .get_bonsai_bookmark(ctx.clone(), &BookmarkName::new("master").unwrap())
            .compat()
            .and_then({
                cloned!(ctx, repo);
                move |maybe_cs_id| {
                    AncestorsNodeStream::new(
                        ctx,
                        &repo.get_changeset_fetcher(),
                        maybe_cs_id.unwrap(),
                    )
                    .collect()
                }
            })
            .and_then({
                cloned!(ctx, repo);
                move |cs_ids| {
                    join_all(cs_ids.into_iter().map({
                        move |cs| {
                            AncestorsNodeStream::new(
                                ctx.clone(),
                                &repo.get_changeset_fetcher(),
                                cs.clone(),
                            )
                            .collect()
                            .map(move |ancestors| {
                                // AncestorsNodeStream incorrectly returns the node itself
                                (cs, ancestors.into_iter().filter(move |anc| *anc != cs))
                            })
                        }
                    }))
                }
            })
            .and_then(move |cs_and_ancestors| {
                let cs_ancestor_map: HashMap<ChangesetId, HashSet<ChangesetId>> = cs_and_ancestors
                    .into_iter()
                    .map(|(cs, ancestors)| (cs, HashSet::from_iter(ancestors)))
                    .collect();

                let mut res = vec![];
                let sli = Arc::new(sli);
                for anc in cs_ancestor_map.keys() {
                    for desc in cs_ancestor_map.keys() {
                        cloned!(ctx, repo, anc, desc, sli);
                        let expected_and_params = (
                            cs_ancestor_map.get(&desc).unwrap().contains(&anc),
                            (anc, desc),
                        );
                        res.push(
                            async move {
                                Ok((
                                    sli.is_ancestor(&ctx, &repo.get_changeset_fetcher(), anc, desc)
                                        .await?,
                                    expected_and_params,
                                ))
                            }
                            .boxed()
                            .compat()
                            .boxify(),
                        );
                    }
                }
                join_all(res).map(|res| {
                    res.into_iter()
                        .all(|(actual, (expected, _))| actual == expected)
                })
            });

        assert!(f.compat().await.unwrap());
    }

    async fn test_is_ancestor_merge_uneven(ctx: CoreContext, repo: BlobRepo, sli: SkiplistIndex) {
        test_is_ancestor(ctx, repo, sli).await;
    }

    async fn test_is_ancestor_unshared_merge_even(
        ctx: CoreContext,
        repo: BlobRepo,
        sli: SkiplistIndex,
    ) {
        test_is_ancestor(ctx, repo, sli).await;
    }

    async fn test_lca_branch_even(ctx: CoreContext, repo: BlobRepo, sli: SkiplistIndex) {
        test_lca(
            ctx,
            repo,
            sli,
            "4f7f3fd428bec1a48f9314414b063c706d9c1aed",
            "16839021e338500b3cf7c9b871c8a07351697d68",
            Some("15c40d0abc36d47fb51c8eaec51ac7aad31f669c"),
        )
        .await;
    }

    async fn test_lca_branch_uneven(ctx: CoreContext, repo: BlobRepo, sli: SkiplistIndex) {
        test_lca(
            ctx,
            repo,
            sli,
            "264f01429683b3dd8042cb3979e8bf37007118bc",
            "16839021e338500b3cf7c9b871c8a07351697d68",
            Some("15c40d0abc36d47fb51c8eaec51ac7aad31f669c"),
        )
        .await;
    }

    async fn test_lca_branch_uneven_with_ancestor(
        ctx: CoreContext,
        repo: BlobRepo,
        sli: SkiplistIndex,
    ) {
        test_lca(
            ctx,
            repo,
            sli,
            "264f01429683b3dd8042cb3979e8bf37007118bc",
            "fc2cef43395ff3a7b28159007f63d6529d2f41ca",
            Some("fc2cef43395ff3a7b28159007f63d6529d2f41ca"),
        )
        .await;
    }

    async fn test_lca_branch_wide_common_parent(
        ctx: CoreContext,
        repo: BlobRepo,
        sli: SkiplistIndex,
    ) {
        test_lca(
            ctx,
            repo,
            sli,
            "04decbb0d1a65789728250ddea2fe8d00248e01c",
            "49f53ab171171b3180e125b918bd1cf0af7e5449",
            Some("4685e9e62e4885d477ead6964a7600c750e39b03"),
        )
        .await;
    }

    async fn test_lca_branch_wide(ctx: CoreContext, repo: BlobRepo, sli: SkiplistIndex) {
        test_lca(
            ctx,
            repo,
            sli,
            "b6a8169454af58b4b72b3665f9aa0d25529755ff",
            "49f53ab171171b3180e125b918bd1cf0af7e5449",
            Some("ecba698fee57eeeef88ac3dcc3b623ede4af47bd"),
        )
        .await;
    }

    async fn test_lca_unshared_merge_even_some_result(
        ctx: CoreContext,
        repo: BlobRepo,
        sli: SkiplistIndex,
    ) {
        test_lca(
            ctx,
            repo,
            sli,
            "7fe9947f101acb4acf7d945e69f0d6ce76a81113",
            "eee492dcdeaae18f91822c4359dd516992e0dbcd",
            Some("eee492dcdeaae18f91822c4359dd516992e0dbcd"),
        )
        .await;
    }

    async fn test_lca_unshared_merge_even_empty_result(
        ctx: CoreContext,
        repo: BlobRepo,
        sli: SkiplistIndex,
    ) {
        test_lca(
            ctx,
            repo,
            sli,
            "eee492dcdeaae18f91822c4359dd516992e0dbcd",
            "0b94a2881dda90f0d64db5fae3ee5695a38e7c8f",
            None,
        )
        .await;
    }

    async fn test_lca_first_generation(ctx: CoreContext, repo: BlobRepo, sli: SkiplistIndex) {
        test_lca(
            ctx,
            repo,
            sli,
            "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536",
            "607314ef579bd2407752361ba1b0c1729d08b281",
            Some("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536"),
        )
        .await;
    }

    async fn test_find_merges_negative(ctx: CoreContext, repo: BlobRepo, sli: SkiplistIndex) {
        test_find_merge(
            ctx,
            repo,
            sli,
            "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536",
            "79a13814c5ce7330173ec04d279bf95ab3f652fb",
            None,
        )
        .await;
    }

    async fn test_find_merges_positive(ctx: CoreContext, repo: BlobRepo, sli: SkiplistIndex) {
        test_find_merge(
            ctx,
            repo,
            sli,
            "d7542c9db7f4c77dab4b315edd328edf1514952f",
            "1f6bc010883e397abeca773192f3370558ee1320",
            Some("1f6bc010883e397abeca773192f3370558ee1320"),
        )
        .await;
    }

    #[fbinit::test]
    async fn test_index_update(fb: FacebookInit) {
        // This test was created to show the problem we had with skiplists not being correctly
        // updated after being trimmed.  The skiplist update algorithm wasn't designed with
        // trimming in mind and assumes that entries pointing closer are always awalable.
        // Resulting skiplits point further away than it's needed (if the worst case of skiplists
        // being updated very often to the latest merge commit).
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(Linear::getrepo(fb).await);
        let sli = SkiplistIndex::with_skip_edge_count(4);

        let old_head =
            string_to_bonsai(&ctx, &repo, "3c15267ebf11807f3d772eb891272b911ec68759").await;
        let new_head =
            string_to_bonsai(&ctx, &repo, "79a13814c5ce7330173ec04d279bf95ab3f652fb").await;

        // This test simulates incremental index update by indexing up to the old_head first and
        // then updating to a new_head
        sli.add_node(&ctx, &repo.get_changeset_fetcher(), old_head, 100)
            .await
            .unwrap();

        sli.trim_to_single_entry_per_changeset();

        sli.add_node(&ctx, &repo.get_changeset_fetcher(), new_head, 100)
            .await
            .unwrap();

        assert_eq!(
            sli.get_skip_edges(new_head)
                .unwrap()
                .into_iter()
                .map(|(_, gen)| gen.value())
                .collect::<Vec<_>>(),
            vec![10, 9, 7, 3]
        );
    }

    #[fbinit::test]
    async fn test_max_skips(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(Linear::getrepo(fb).await);
        let sli = SkiplistIndex::new();
        let src_node =
            string_to_bonsai(&ctx, &repo, "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157").await;
        let dst_node =
            string_to_bonsai(&ctx, &repo, "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").await;
        let cs_fetcher = repo.get_changeset_fetcher();
        sli.add_node(&ctx, &cs_fetcher, src_node, 10).await?;

        let f = sli.query_reachability(&ctx, &cs_fetcher, src_node, dst_node);

        let tunables = tunables::MononokeTunables::default();
        tunables.update_ints(&hashmap! {"skiplist_max_skips_without_yield".to_string() => 1});
        tunables::with_tunables_async(tunables, f).await?;

        Ok(())
    }

    skiplist_test!(test_lca_first_generation, Linear);
    skiplist_test!(query_reachability_hint_on_self_is_true, Linear);
    skiplist_test!(query_reachability_to_higher_gen_is_false, Linear);
    skiplist_test!(query_from_indexed_merge_node, UnsharedMergeEven);
    skiplist_test!(advance_node_linear, Linear);
    skiplist_test!(advance_node_uneven_merge, MergeUneven);
    skiplist_test!(advance_node_on_partial_index, MergeUneven);
    skiplist_test!(simul_node_advance_on_wide_branch, BranchWide);
    skiplist_test!(process_frontier_on_wide_branch, BranchWide);
    skiplist_test!(test_is_ancestor_merge_uneven, MergeUneven);
    skiplist_test!(test_is_ancestor_unshared_merge_even, UnsharedMergeEven);
    skiplist_test!(test_lca_branch_even, BranchEven);
    skiplist_test!(test_lca_branch_uneven, BranchUneven);
    skiplist_test!(test_lca_branch_uneven_with_ancestor, BranchUneven);
    skiplist_test!(test_lca_branch_wide_common_parent, BranchWide);
    skiplist_test!(test_lca_branch_wide, BranchWide);
    skiplist_test!(test_lca_unshared_merge_even_some_result, UnsharedMergeEven);
    skiplist_test!(test_lca_unshared_merge_even_empty_result, UnsharedMergeEven);
    skiplist_test!(test_find_merges_negative, Linear);
    skiplist_test!(test_find_merges_positive, MergeEven);
}
