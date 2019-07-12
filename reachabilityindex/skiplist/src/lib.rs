// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate failure_ext as failure;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bytes::Bytes;
use chashmap::CHashMap;
use cloned::cloned;
use context::CoreContext;
use failure_ext::{Error, Result};
use futures::future::{join_all, loop_fn, ok, Future, Loop};
use futures::IntoFuture;
use futures_ext::{BoxFuture, FutureExt};
use maplit::{hashmap, hashset};

use changeset_fetcher::ChangesetFetcher;
use mononoke_types::{ChangesetId, Generation};

use common::{
    advance_bfs_layer, changesets_with_generation_numbers, check_if_node_exists,
    fetch_generation_and_join, get_parents,
};
use reachabilityindex::{errors::*, LeastCommonAncestorsHint, NodeFrontier, ReachabilityIndex};
use skiplist_thrift;

use rust_thrift::compact_protocol;

const DEFAULT_EDGE_COUNT: u32 = 10;

// Each indexed node fits into one of two categories:
// - It has skiplist edges
// - It only has edges to its parents.
#[derive(Clone)]
pub enum SkiplistNodeType {
    // A list of skip edges which keep doubling
    // in distance from their root node.
    // The ith skip edge is at most 2^i commits away.
    SkipEdges(Vec<(ChangesetId, Generation)>),
    ParentEdges(Vec<(ChangesetId, Generation)>),
}

impl SkiplistNodeType {
    pub fn to_thrift(&self) -> skiplist_thrift::SkiplistNodeType {
        fn encode_vec_to_thrift(
            cs_gen: Vec<(ChangesetId, Generation)>,
        ) -> Vec<skiplist_thrift::CommitAndGenerationNumber> {
            cs_gen
                .into_iter()
                .map(|(cs_id, gen_num)| {
                    let cs_id = cs_id.into_thrift();
                    let gen = skiplist_thrift::GenerationNum(gen_num.value() as i64);
                    skiplist_thrift::CommitAndGenerationNumber { cs_id, gen }
                })
                .collect()
        }

        match self {
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
                decode_vec_to_thrift(thrift_edges.edges).map(SkiplistNodeType::SkipEdges)
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

pub fn deserialize_skiplist_map(bytes: Bytes) -> Result<HashMap<ChangesetId, SkiplistNodeType>> {
    let map: HashMap<_, skiplist_thrift::SkiplistNodeType> = compact_protocol::deserialize(&bytes)?;

    let v: Result<Vec<_>> = map
        .into_iter()
        .map(|(cs_id, skiplist_thrift)| {
            ChangesetId::from_thrift(cs_id).map(|cs_id| {
                SkiplistNodeType::from_thrift(skiplist_thrift)
                    .map(move |skiplist| (cs_id, skiplist))
            })
        })
        .collect();

    v?.into_iter().collect()
}

struct SkiplistEdgeMapping {
    pub mapping: CHashMap<ChangesetId, SkiplistNodeType>,
    pub skip_edges_per_node: u32,
}

impl SkiplistEdgeMapping {
    pub fn new() -> Self {
        SkiplistEdgeMapping {
            mapping: CHashMap::new(),
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

fn nth_node_or_last<T: Clone>(v: &Vec<T>, i: usize) -> Option<T> {
    return v.get(i).or(v.last()).cloned();
}

/// compute the skip list edges which start by pointing at start_node.
/// goes as far as possible, if an unindexed or merge node are reached then skip edges
/// will not go past that node.
/// note that start node is not the node that we'll be adding these skip list edges to.
/// it is the first node that we'll consider as a candidate for a skip list edge.
/// hence it should always be the parent of the node we are creating edges for.
fn compute_skip_edges(
    start_node: (ChangesetId, Generation),
    skip_edge_mapping: Arc<SkiplistEdgeMapping>,
) -> Vec<(ChangesetId, Generation)> {
    let mut curr = start_node;

    let max_skip_edge_count = skip_edge_mapping.skip_edges_per_node as usize;
    let mut skip_edges = vec![curr];
    let mut i: usize = 0;

    while let Some(read_locked_entry) = skip_edge_mapping.mapping.get(&curr.0) {
        if let SkiplistNodeType::SkipEdges(edges) = &*read_locked_entry {
            if let Some(next_node) = nth_node_or_last(edges, i) {
                curr = next_node;
                skip_edges.push(curr);
                if skip_edges.len() >= max_skip_edge_count {
                    break;
                }
            } else {
                break;
            }
        } else {
            break;
        }
        i += 1;
    }
    skip_edges
}
/// Structure for indexing skip list edges for reachability queries.
#[derive(Clone)]
pub struct SkiplistIndex {
    // Each hash that the structure knows about is mapped to a  collection
    // of (Gen, Hash) pairs, wrapped in an enum. The semantics behind this are:
    // - If the hash isn't in the hash map, the node hasn't been indexed yet.
    // - If the enum type is SkipEdges, then we can safely traverse the longest
    //   edge that doesn't pass the generation number of the destination.
    // - If the enum type is ParentEdges, then we couldn't safely add skip edges
    //   from this node (which is always the case for a merge node), so we must
    //   recurse on all the children.
    skip_list_edges: Arc<SkiplistEdgeMapping>,
}

// Find nodes to index during lazy indexing
// This method searches backwards from a start node until a specified depth,
// collecting all nodes which are not currently present in the index.
// Then it orders them topologically using their generation numbers and returns them.
fn find_nodes_to_index(
    ctx: CoreContext,
    changeset_fetcher: Arc<dyn ChangesetFetcher>,
    skip_list_edges: Arc<SkiplistEdgeMapping>,
    (start_node, start_gen): (ChangesetId, Generation),
    depth: u64,
) -> BoxFuture<Vec<(ChangesetId, Generation)>, Error> {
    let start_bfs_layer: HashSet<_> = vec![(start_node, start_gen)].into_iter().collect();
    let start_seen: HashSet<_> = HashSet::new();
    let ancestors_to_depth =
        check_if_node_exists(ctx.clone(), changeset_fetcher.clone(), start_node.clone()).and_then(
            move |_| {
                loop_fn(
                    (start_bfs_layer, start_seen, depth),
                    move |(curr_layer_unfiltered, curr_seen, curr_depth)| {
                        let curr_layer: HashSet<_> = curr_layer_unfiltered
                            .into_iter()
                            .filter(|(hash, _gen)| !skip_list_edges.mapping.contains_key(&hash))
                            .collect();

                        if curr_depth == 0 || curr_layer.is_empty() {
                            ok(Loop::Break(curr_seen)).boxify()
                        } else {
                            advance_bfs_layer(
                                ctx.clone(),
                                changeset_fetcher.clone(),
                                curr_layer,
                                curr_seen,
                            )
                            .map(move |(next_layer, next_seen)| {
                                Loop::Continue((next_layer, next_seen, curr_depth - 1))
                            })
                            .boxify()
                        }
                    },
                )
            },
        );
    ancestors_to_depth
        .map(|hash_gen_pairs| {
            let mut top_order = hash_gen_pairs.into_iter().collect::<Vec<_>>();
            top_order.sort_by(|a, b| (a.1).cmp(&b.1));
            top_order
        })
        .from_err()
        .boxify()
}

/// From a starting node, index all nodes that are reachable within a given distance.
/// If a previously indexed node is reached, indexing will stop there.
fn lazy_index_node(
    ctx: CoreContext,
    changeset_fetcher: Arc<dyn ChangesetFetcher>,
    skip_edge_mapping: Arc<SkiplistEdgeMapping>,
    node: ChangesetId,
    max_depth: u64,
) -> BoxFuture<(), Error> {
    // if this node is indexed or we've passed the max depth, return
    if max_depth == 0 || skip_edge_mapping.mapping.contains_key(&node) {
        ok(()).boxify()
    } else {
        fetch_generation_and_join(ctx.clone(), changeset_fetcher.clone(), node)
            .and_then({
                cloned!(ctx, changeset_fetcher, skip_edge_mapping);
                move |node_gen_pair| {
                    find_nodes_to_index(
                        ctx,
                        changeset_fetcher,
                        skip_edge_mapping,
                        node_gen_pair,
                        max_depth,
                    )
                }
            })
            .and_then({
                move |node_gen_pairs| {
                    join_all(node_gen_pairs.into_iter().map({
                        cloned!(changeset_fetcher);
                        move |(hash, _gen)| {
                            get_parents(ctx.clone(), changeset_fetcher.clone(), hash.clone())
                                .and_then({
                                    cloned!(ctx, changeset_fetcher);
                                    move |parents| {
                                        changesets_with_generation_numbers(
                                            ctx,
                                            changeset_fetcher,
                                            parents,
                                        )
                                    }
                                })
                                .map(move |parent_gen_pairs| (hash, parent_gen_pairs))
                        }
                    }))
                }
            })
            .map(move |hash_parentgens_gen_vec| {
                // determine what kind of skip edges to add for this node
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
                        let new_edges =
                            compute_skip_edges(unique_parent_gen_pair, skip_edge_mapping.clone());
                        skip_edge_mapping
                            .mapping
                            .insert(curr_hash, SkiplistNodeType::SkipEdges(new_edges));
                    }
                }
            })
            .boxify()
    }
}

impl SkiplistIndex {
    pub fn new() -> Self {
        SkiplistIndex {
            skip_list_edges: Arc::new(SkiplistEdgeMapping::new()),
        }
    }

    pub fn new_with_skiplist_graph(skiplist_graph: HashMap<ChangesetId, SkiplistNodeType>) -> Self {
        let s = Self::new();
        for (key, value) in skiplist_graph {
            s.skip_list_edges.mapping.insert(key, value);
        }
        s
    }

    pub fn with_skip_edge_count(skip_edges_per_node: u32) -> Self {
        SkiplistIndex {
            skip_list_edges: Arc::new(
                SkiplistEdgeMapping::new().with_skip_edge_count(skip_edges_per_node),
            ),
        }
    }

    pub fn skip_edge_count(&self) -> u32 {
        self.skip_list_edges.skip_edges_per_node
    }

    pub fn add_node(
        &self,
        ctx: CoreContext,
        changeset_fetcher: Arc<dyn ChangesetFetcher>,
        node: ChangesetId,
        max_index_depth: u64,
    ) -> BoxFuture<(), Error> {
        lazy_index_node(
            ctx,
            changeset_fetcher,
            self.skip_list_edges.clone(),
            node,
            max_index_depth,
        )
    }

    /// get skiplist edges originating from a particular node hash
    /// returns Some(edges) if this node was indexed with skip edges
    /// returns None if this node was unindexed, or was indexed with parent edges only.
    pub fn get_skip_edges(&self, node: ChangesetId) -> Option<Vec<(ChangesetId, Generation)>> {
        if let Some(read_guard) = self.skip_list_edges.mapping.get(&node) {
            if let SkiplistNodeType::SkipEdges(edges) = &*read_guard {
                Some(edges.clone())
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn get_all_skip_edges(&self) -> HashMap<ChangesetId, SkiplistNodeType> {
        self.skip_list_edges.mapping.clone().into_iter().collect()
    }

    pub fn is_node_indexed(&self, node: ChangesetId) -> bool {
        self.skip_list_edges.mapping.contains_key(&node)
    }

    pub fn indexed_node_count(&self) -> usize {
        self.skip_list_edges.mapping.len()
    }
}

impl ReachabilityIndex for SkiplistIndex {
    fn query_reachability(
        &self,
        ctx: CoreContext,
        changeset_fetcher: Arc<dyn ChangesetFetcher>,
        desc_hash: ChangesetId,
        anc_hash: ChangesetId,
    ) -> BoxFuture<bool, Error> {
        cloned!(self.skip_list_edges);
        fetch_generation_and_join(ctx.clone(), changeset_fetcher.clone(), desc_hash)
            .join(fetch_generation_and_join(
                ctx.clone(),
                changeset_fetcher.clone(),
                anc_hash,
            ))
            .and_then(move |((desc_hash, desc_gen), (anc_hash, anc_gen))| {
                ctx.perf_counters()
                    .set_counter("ancestor_gen", anc_gen.value() as i64);
                ctx.perf_counters()
                    .set_counter("descendant_gen", desc_gen.value() as i64);

                process_frontier(
                    ctx.clone(),
                    changeset_fetcher,
                    skip_list_edges,
                    NodeFrontier::new(hashmap! {desc_gen => hashset!{desc_hash}}),
                    anc_gen,
                )
                .map(move |frontier| {
                    match frontier.get_all_changesets_for_gen_num(anc_gen) {
                        Some(cs_ids) => cs_ids.contains(&anc_hash),
                        None => false,
                    }
                })
            })
            .boxify()
    }
}

// Take all changesets from `all_cs_ids` that have skiplist edges in `skip_edges` and moves them.
// Returns changesets that wasn't moved and a NodeFrontier of moved nodes
fn move_skippable_nodes(
    skip_edges: Arc<SkiplistEdgeMapping>,
    all_cs_ids: Vec<ChangesetId>,
    gen: Generation,
) -> (Vec<ChangesetId>, NodeFrontier) {
    let mut no_skiplist_edges = vec![];
    let mut node_frontier = NodeFrontier::default();

    for cs_id in all_cs_ids {
        if let Some(read_locked_entry) = skip_edges.mapping.get(&cs_id) {
            match &*read_locked_entry {
                SkiplistNodeType::SkipEdges(edges) => {
                    let best_edge = edges
                        .iter()
                        .take_while(|edge_pair| edge_pair.1 >= gen)
                        .last()
                        .cloned();
                    if let Some(edge_pair) = best_edge {
                        node_frontier.insert(edge_pair);
                    } else {
                        no_skiplist_edges.push(cs_id);
                    }
                }
                SkiplistNodeType::ParentEdges(edges) => {
                    for edge_pair in edges {
                        node_frontier.insert(*edge_pair);
                    }
                }
            }
        } else {
            no_skiplist_edges.push(cs_id);
        }
    }

    (no_skiplist_edges, node_frontier)
}

// Returns a frontier "C" that satisfy the following condition:
/// - Max generation number in "C" is <= gen
/// - Any ancestor of "node_frontier" with generation <= gen is also an ancestor of "C"
fn process_frontier(
    ctx: CoreContext,
    changeset_fetcher: Arc<dyn ChangesetFetcher>,
    skip_edges: Arc<SkiplistEdgeMapping>,
    node_frontier: NodeFrontier,
    max_gen: Generation,
) -> impl Future<Item = NodeFrontier, Error = Error> {
    loop_fn(
        node_frontier,
        move |mut node_frontier: NodeFrontier| match node_frontier.max_gen() {
            Some(val) if val > max_gen => {
                let all_cs_ids = node_frontier.remove_max_gen().unwrap();
                let (no_skiplist_edges, skipped_frontier) = move_skippable_nodes(
                    skip_edges.clone(),
                    all_cs_ids.into_iter().collect(),
                    max_gen,
                );
                if skipped_frontier.len() == 0 {
                    ctx.perf_counters().increment_counter("noskip_iterations");
                } else {
                    ctx.perf_counters().increment_counter("skip_iterations");
                    if let Some(new) = skipped_frontier.max_gen() {
                        ctx.perf_counters().add_to_counter(
                            "skipped_generations",
                            (val.value() - new.value()) as i64,
                        );
                    }
                }
                let parents_futs = no_skiplist_edges.into_iter().map({
                    cloned!(ctx, changeset_fetcher);
                    move |cs_id| {
                        changeset_fetcher
                            .get_parents(ctx.clone(), cs_id)
                            .map(IntoIterator::into_iter)
                    }
                });
                join_all(parents_futs)
                    .map(|all_parents| all_parents.into_iter().flatten())
                    .and_then({
                        cloned!(ctx, changeset_fetcher);
                        move |parents| {
                            let mut gen_futs = vec![];
                            for p in parents {
                                let f = changeset_fetcher
                                    .get_generation_number(ctx.clone(), p)
                                    .map(move |gen_num| (p, gen_num));
                                gen_futs.push(f);
                            }
                            join_all(gen_futs)
                        }
                    })
                    .map(move |gen_cs| {
                        node_frontier.extend(gen_cs);

                        for (gen, s) in skipped_frontier {
                            for entry in s {
                                node_frontier.insert((entry, gen));
                            }
                        }
                        Loop::Continue(node_frontier)
                    })
                    .left_future()
            }
            _ => ok(Loop::Break(node_frontier)).right_future(),
        },
    )
}

impl LeastCommonAncestorsHint for SkiplistIndex {
    fn lca_hint(
        &self,
        ctx: CoreContext,
        changeset_fetcher: Arc<dyn ChangesetFetcher>,
        node_frontier: NodeFrontier,
        gen: Generation,
    ) -> BoxFuture<NodeFrontier, Error> {
        process_frontier(
            ctx,
            changeset_fetcher,
            self.skip_list_edges.clone(),
            node_frontier,
            gen,
        )
        .boxify()
    }

    fn is_ancestor(
        &self,
        ctx: CoreContext,
        changeset_fetcher: Arc<dyn ChangesetFetcher>,
        ancestor: ChangesetId,
        descendant: ChangesetId,
    ) -> BoxFuture<bool, Error> {
        let anc_with_gen = changeset_fetcher
            .get_generation_number(ctx.clone(), ancestor)
            .map(move |gen| (ancestor, gen));
        let desc_with_gen = changeset_fetcher
            .get_generation_number(ctx.clone(), descendant)
            .map(move |gen| (descendant, gen));

        anc_with_gen
            .join(desc_with_gen)
            .and_then({
                let this = self.clone();
                move |(anc, desc)| {
                    if anc.1 >= desc.1 {
                        Ok(false).into_future().boxify()
                    } else {
                        let mut frontier = NodeFrontier::default();
                        frontier.insert(desc);
                        this.lca_hint(ctx.clone(), changeset_fetcher, frontier, anc.1)
                            .map(move |res| {
                                // If "ancestor" is an ancestor of "descendant" lca_hint will return
                                // a node frontier that contains "ancestor".
                                match res.get_all_changesets_for_gen_num(anc.1) {
                                    Some(generation_set) if generation_set.contains(&anc.0) => true,
                                    _ => false,
                                }
                            })
                            .boxify()
                    }
                }
            })
            .boxify()
    }
}

#[cfg(test)]
mod test {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use async_unit;
    use blobrepo::BlobRepo;
    use bookmarks::BookmarkName;
    use chashmap::CHashMap;
    use context::CoreContext;
    use futures::stream::iter_ok;
    use futures::stream::Stream;
    use revset::AncestorsNodeStream;
    use std::collections::HashSet;
    use std::iter::FromIterator;

    use super::*;
    use fixtures::{branch_wide, linear, merge_uneven, unshared_merge_even};
    use test_helpers::string_to_bonsai;
    use test_helpers::test_branch_wide_reachability;
    use test_helpers::test_linear_reachability;
    use test_helpers::test_merge_uneven_reachability;
    use tokio;

    #[test]
    fn simple_init() {
        async_unit::tokio_unit_test(|| {
            let sli = SkiplistIndex::new();
            assert_eq!(sli.skip_edge_count(), DEFAULT_EDGE_COUNT);

            let sli_with_20 = SkiplistIndex::with_skip_edge_count(20);
            assert_eq!(sli_with_20.skip_edge_count(), 20);
        });
    }

    #[test]
    fn arc_chash_is_sync_and_send() {
        fn is_sync<T: Sync>() {}
        fn is_send<T: Send>() {}

        is_sync::<Arc<CHashMap<ChangesetId, SkiplistNodeType>>>();
        is_send::<Arc<CHashMap<ChangesetId, SkiplistNodeType>>>();
    }

    #[test]
    fn test_add_node() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(linear::getrepo(None));
            let sli = SkiplistIndex::new();
            let master_node = string_to_bonsai(
                ctx.clone(),
                &repo,
                "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157",
            );
            sli.add_node(ctx.clone(), repo.get_changeset_fetcher(), master_node, 100)
                .wait()
                .unwrap();
            let ordered_hashes = vec![
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "0ed509bf086fadcb8a8a5384dc3b550729b0fc17",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "cb15ca4a43a59acff5388cea9648c162afde8372",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "d0a361e9022d226ae52f689667bd7d212a19cfe0",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "607314ef579bd2407752361ba1b0c1729d08b281",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "3e0e761030db6e479a7fb58b12881883f9f8c63f",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536",
                ),
            ];
            assert_eq!(sli.indexed_node_count(), ordered_hashes.len());
            for node in ordered_hashes.into_iter() {
                assert!(sli.is_node_indexed(node));
            }
        });
    }

    #[test]
    fn test_skip_edges_reach_end_in_linear() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(linear::getrepo(None));
            let sli = SkiplistIndex::new();
            let master_node = string_to_bonsai(
                ctx.clone(),
                &repo,
                "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157",
            );
            sli.add_node(ctx.clone(), repo.get_changeset_fetcher(), master_node, 100)
                .wait()
                .unwrap();
            let ordered_hashes = vec![
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "0ed509bf086fadcb8a8a5384dc3b550729b0fc17",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "cb15ca4a43a59acff5388cea9648c162afde8372",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "d0a361e9022d226ae52f689667bd7d212a19cfe0",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "607314ef579bd2407752361ba1b0c1729d08b281",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "3e0e761030db6e479a7fb58b12881883f9f8c63f",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536",
                ),
            ];
            assert_eq!(sli.indexed_node_count(), ordered_hashes.len());
            for node in ordered_hashes.into_iter() {
                assert!(sli.is_node_indexed(node));
                if node
                    != string_to_bonsai(
                        ctx.clone(),
                        &repo,
                        "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536",
                    )
                {
                    let skip_edges: Vec<_> = sli
                        .get_skip_edges(node)
                        .unwrap()
                        .into_iter()
                        .map(|(node, _)| node)
                        .collect();
                    assert!(skip_edges.contains(&string_to_bonsai(
                        ctx.clone(),
                        &repo,
                        "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536"
                    )));
                }
            }
        });
    }

    #[test]
    fn test_skip_edges_progress_powers_of_2() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(linear::getrepo(None));
            let sli = SkiplistIndex::new();
            let master_node = string_to_bonsai(
                ctx.clone(),
                &repo,
                "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157",
            );
            sli.add_node(ctx.clone(), repo.get_changeset_fetcher(), master_node, 100)
                .wait()
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
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "0ed509bf086fadcb8a8a5384dc3b550729b0fc17",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "d0a361e9022d226ae52f689667bd7d212a19cfe0",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536",
                ),
            ];
            assert_eq!(skip_edges, expected_hashes);
        });
    }

    #[test]
    fn test_skip_edges_reach_end_in_merge() {
        async_unit::tokio_unit_test(move || {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(merge_uneven::getrepo(None));
            let root_node = string_to_bonsai(
                ctx.clone(),
                &repo,
                "15c40d0abc36d47fb51c8eaec51ac7aad31f669c",
            );

            // order is oldest to newest
            let branch_1 = vec![
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "3cda5c78aa35f0f5b09780d971197b51cad4613a",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "1d8a907f7b4bf50c6a09c16361e2205047ecc5e5",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "16839021e338500b3cf7c9b871c8a07351697d68",
                ),
            ];

            // order is oldest to newest
            let branch_2 = vec![
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "d7542c9db7f4c77dab4b315edd328edf1514952f",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "b65231269f651cfe784fd1d97ef02a049a37b8a0",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "4f7f3fd428bec1a48f9314414b063c706d9c1aed",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "795b8133cf375f6d68d27c6c23db24cd5d0cd00f",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "bc7b4d0f858c19e2474b03e442b8495fd7aeef33",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "fc2cef43395ff3a7b28159007f63d6529d2f41ca",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "5d43888a3c972fe68c224f93d41b30e9f888df7c",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "264f01429683b3dd8042cb3979e8bf37007118bc",
                ),
            ];

            let merge_node = string_to_bonsai(
                ctx.clone(),
                &repo,
                "7221fa26c85f147db37c2b5f4dbcd5fe52e7645b",
            );
            let sli = SkiplistIndex::new();
            sli.add_node(ctx.clone(), repo.get_changeset_fetcher(), merge_node, 100)
                .wait()
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
        });
    }

    #[test]
    fn test_partial_index_in_merge() {
        async_unit::tokio_unit_test(move || {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(merge_uneven::getrepo(None));
            let root_node = string_to_bonsai(
                ctx.clone(),
                &repo,
                "15c40d0abc36d47fb51c8eaec51ac7aad31f669c",
            );

            // order is oldest to newest
            let branch_1 = vec![
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "3cda5c78aa35f0f5b09780d971197b51cad4613a",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "1d8a907f7b4bf50c6a09c16361e2205047ecc5e5",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "16839021e338500b3cf7c9b871c8a07351697d68",
                ),
            ];

            let branch_1_head = string_to_bonsai(
                ctx.clone(),
                &repo,
                "16839021e338500b3cf7c9b871c8a07351697d68",
            );

            // order is oldest to newest
            let branch_2 = vec![
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "d7542c9db7f4c77dab4b315edd328edf1514952f",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "b65231269f651cfe784fd1d97ef02a049a37b8a0",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "4f7f3fd428bec1a48f9314414b063c706d9c1aed",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "795b8133cf375f6d68d27c6c23db24cd5d0cd00f",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "bc7b4d0f858c19e2474b03e442b8495fd7aeef33",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "fc2cef43395ff3a7b28159007f63d6529d2f41ca",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "5d43888a3c972fe68c224f93d41b30e9f888df7c",
                ),
                string_to_bonsai(
                    ctx.clone(),
                    &repo,
                    "264f01429683b3dd8042cb3979e8bf37007118bc",
                ),
            ];
            let branch_2_head = string_to_bonsai(
                ctx.clone(),
                &repo,
                "264f01429683b3dd8042cb3979e8bf37007118bc",
            );

            let _merge_node = string_to_bonsai(
                ctx.clone(),
                &repo,
                "7221fa26c85f147db37c2b5f4dbcd5fe52e7645b",
            );
            let sli = SkiplistIndex::new();

            // index just one branch first
            sli.add_node(
                ctx.clone(),
                repo.get_changeset_fetcher(),
                branch_1_head,
                100,
            )
            .wait()
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
            sli.add_node(
                ctx.clone(),
                repo.get_changeset_fetcher(),
                branch_2_head,
                100,
            )
            .wait()
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
        });
    }

    #[test]
    fn test_simul_index_on_wide_branch() {
        async_unit::tokio_unit_test(move || {
            let ctx = CoreContext::test_mock();
            // this repo has no merges but many branches
            let repo = Arc::new(branch_wide::getrepo(None));
            let root_node = string_to_bonsai(
                ctx.clone(),
                &repo,
                "ecba698fee57eeeef88ac3dcc3b623ede4af47bd",
            );

            let b1 = string_to_bonsai(
                ctx.clone(),
                &repo,
                "9e8521affb7f9d10e9551a99c526e69909042b20",
            );
            let b2 = string_to_bonsai(
                ctx.clone(),
                &repo,
                "4685e9e62e4885d477ead6964a7600c750e39b03",
            );
            let b1_1 = string_to_bonsai(
                ctx.clone(),
                &repo,
                "b6a8169454af58b4b72b3665f9aa0d25529755ff",
            );
            let b1_2 = string_to_bonsai(
                ctx.clone(),
                &repo,
                "c27ef5b7f15e9930e5b93b1f32cc2108a2aabe12",
            );
            let b2_1 = string_to_bonsai(
                ctx.clone(),
                &repo,
                "04decbb0d1a65789728250ddea2fe8d00248e01c",
            );
            let b2_2 = string_to_bonsai(
                ctx.clone(),
                &repo,
                "49f53ab171171b3180e125b918bd1cf0af7e5449",
            );

            let sli = SkiplistIndex::new();
            iter_ok::<_, Error>(vec![b1_1, b1_2, b2_1, b2_2])
                .map(|branch_tip| {
                    sli.add_node(ctx.clone(), repo.get_changeset_fetcher(), branch_tip, 100)
                })
                .buffered(4)
                .for_each(|_| ok(()))
                .wait()
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
        });
    }

    #[test]
    fn linear_reachability() {
        let sli_constructor = || SkiplistIndex::new();
        test_linear_reachability(sli_constructor);
    }

    #[test]
    fn merge_uneven_reachability() {
        let sli_constructor = || SkiplistIndex::new();
        test_merge_uneven_reachability(sli_constructor);
    }

    #[test]
    fn branch_wide_reachability() {
        let sli_constructor = || SkiplistIndex::new();
        test_branch_wide_reachability(sli_constructor);
    }

    struct CountingChangesetFetcher {
        pub get_parents_count: Arc<AtomicUsize>,
        pub get_gen_number_count: Arc<AtomicUsize>,
        cs_fetcher: Arc<dyn ChangesetFetcher>,
    }

    impl CountingChangesetFetcher {
        fn new(
            cs_fetcher: Arc<dyn ChangesetFetcher>,
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

    impl ChangesetFetcher for CountingChangesetFetcher {
        fn get_generation_number(
            &self,
            ctx: CoreContext,
            cs_id: ChangesetId,
        ) -> BoxFuture<Generation, Error> {
            self.get_gen_number_count.fetch_add(1, Ordering::Relaxed);
            self.cs_fetcher.get_generation_number(ctx, cs_id)
        }

        fn get_parents(
            &self,
            ctx: CoreContext,
            cs_id: ChangesetId,
        ) -> BoxFuture<Vec<ChangesetId>, Error> {
            self.get_parents_count.fetch_add(1, Ordering::Relaxed);
            self.cs_fetcher.get_parents(ctx, cs_id)
        }
    }

    fn run_future<F, I>(runtime: &mut tokio::runtime::Runtime, future: F) -> Result<I>
    where
        F: Future<Item = I, Error = Error> + Send + 'static,
        I: Send + 'static,
    {
        runtime.block_on(future)
    }

    fn query_reachability_hint_on_self_is_true(
        runtime: &mut tokio::runtime::Runtime,
        ctx: CoreContext,
        repo: Arc<BlobRepo>,
        sli: SkiplistIndex,
    ) {
        let mut ordered_hashes_oldest_to_newest = vec![
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "0ed509bf086fadcb8a8a5384dc3b550729b0fc17",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "cb15ca4a43a59acff5388cea9648c162afde8372",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "d0a361e9022d226ae52f689667bd7d212a19cfe0",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "607314ef579bd2407752361ba1b0c1729d08b281",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "3e0e761030db6e479a7fb58b12881883f9f8c63f",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536",
            ),
        ];
        ordered_hashes_oldest_to_newest.reverse();
        // indexing doesn't even take place if the query can conclude true or false right away

        for (_, node) in ordered_hashes_oldest_to_newest.into_iter().enumerate() {
            let f = sli.query_reachability(ctx.clone(), repo.get_changeset_fetcher(), node, node);
            assert!(run_future(runtime, f).unwrap());
        }
    }

    macro_rules! skiplist_test {
        ($test_name:ident, $repo:ident) => {
            mod $test_name {
                use super::*;
                #[test]
                fn no_index() {
                    let mut runtime = tokio::runtime::Runtime::new().unwrap();
                    let ctx = CoreContext::test_mock();
                    let repo = Arc::new($repo::getrepo(None));
                    let sli = SkiplistIndex::new();
                    $test_name(&mut runtime, ctx, repo, sli)
                }

                #[test]
                fn all_indexed() {
                    let ctx = CoreContext::test_mock();
                    let repo = Arc::new($repo::getrepo(None));
                    let sli = SkiplistIndex::new();
                    {
                        let mut runtime = tokio::runtime::Runtime::new().unwrap();
                        let heads = repo.get_bonsai_heads_maybe_stale(ctx.clone()).collect();
                        let heads = run_future(&mut runtime, heads).unwrap();
                        for head in heads {
                            let f =
                                sli.add_node(ctx.clone(), repo.get_changeset_fetcher(), head, 100);
                            run_future(&mut runtime, f).unwrap();
                        }
                    }
                    let mut runtime = tokio::runtime::Runtime::new().unwrap();
                    $test_name(&mut runtime, ctx, repo, sli);
                }
            }
        };
    }

    fn query_reachability_to_higher_gen_is_false(
        runtime: &mut tokio::runtime::Runtime,
        ctx: CoreContext,
        repo: Arc<BlobRepo>,
        sli: SkiplistIndex,
    ) {
        let mut ordered_hashes_oldest_to_newest = vec![
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "0ed509bf086fadcb8a8a5384dc3b550729b0fc17",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "cb15ca4a43a59acff5388cea9648c162afde8372",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "d0a361e9022d226ae52f689667bd7d212a19cfe0",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "607314ef579bd2407752361ba1b0c1729d08b281",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "3e0e761030db6e479a7fb58b12881883f9f8c63f",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536",
            ),
        ];
        ordered_hashes_oldest_to_newest.reverse();

        // indexing doesn't even take place if the query can conclude true or false right away
        for i in 0..ordered_hashes_oldest_to_newest.len() {
            let src_node = ordered_hashes_oldest_to_newest.get(i).unwrap();
            for j in i + 1..ordered_hashes_oldest_to_newest.len() {
                let dst_node = ordered_hashes_oldest_to_newest.get(j).unwrap();
                let f = sli.query_reachability(
                    ctx.clone(),
                    repo.get_changeset_fetcher(),
                    *src_node,
                    *dst_node,
                );
                assert!(!run_future(runtime, f).unwrap());
            }
        }
    }

    #[test]
    fn test_query_reachability_from_unindexed_node() {
        let mut runtime = tokio::runtime::Runtime::new().unwrap();
        let ctx = CoreContext::test_mock();
        let repo = Arc::new(linear::getrepo(None));
        let sli = SkiplistIndex::new();
        let get_parents_count = Arc::new(AtomicUsize::new(0));
        let get_gen_number_count = Arc::new(AtomicUsize::new(0));
        let cs_fetcher = Arc::new(CountingChangesetFetcher::new(
            repo.get_changeset_fetcher(),
            get_parents_count.clone(),
            get_gen_number_count,
        ));

        let src_node = string_to_bonsai(
            ctx.clone(),
            &repo,
            "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157",
        );
        let dst_node = string_to_bonsai(
            ctx.clone(),
            &repo,
            "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536",
        );
        let f = sli.query_reachability(ctx.clone(), cs_fetcher.clone(), src_node, dst_node);
        assert!(run_future(&mut runtime, f).unwrap());
        let ordered_hashes = vec![
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "0ed509bf086fadcb8a8a5384dc3b550729b0fc17",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "cb15ca4a43a59acff5388cea9648c162afde8372",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "d0a361e9022d226ae52f689667bd7d212a19cfe0",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "607314ef579bd2407752361ba1b0c1729d08b281",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "3e0e761030db6e479a7fb58b12881883f9f8c63f",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536",
            ),
        ];
        // Nothing is indexed by default
        assert_eq!(sli.indexed_node_count(), 0);
        for node in ordered_hashes.iter() {
            assert!(!sli.is_node_indexed(*node));
        }

        let parents_count_before_indexing = get_parents_count.load(Ordering::Relaxed);
        assert!(parents_count_before_indexing > 0);

        // Index
        sli.add_node(ctx.clone(), repo.get_changeset_fetcher(), src_node, 10)
            .wait()
            .unwrap();
        assert_eq!(sli.indexed_node_count(), ordered_hashes.len());
        for node in ordered_hashes.into_iter() {
            assert!(sli.is_node_indexed(node));
        }

        // Make sure that we don't use changeset fetcher anymore, because everything is
        // indexed
        let f = sli.query_reachability(ctx.clone(), cs_fetcher, src_node, dst_node);

        assert!(run_future(&mut runtime, f).unwrap());

        assert_eq!(
            parents_count_before_indexing,
            get_parents_count.load(Ordering::Relaxed)
        );
    }

    fn query_from_indexed_merge_node(
        runtime: &mut tokio::runtime::Runtime,
        ctx: CoreContext,
        repo: Arc<BlobRepo>,
        sli: SkiplistIndex,
    ) {
        let merge_node = string_to_bonsai(
            ctx.clone(),
            &repo,
            "d592490c4386cdb3373dd93af04d563de199b2fb",
        );
        let commit_after_merge = string_to_bonsai(
            ctx.clone(),
            &repo,
            "7fe9947f101acb4acf7d945e69f0d6ce76a81113",
        );
        // Indexing starting from a merge node
        run_future(
            runtime,
            sli.add_node(
                ctx.clone(),
                repo.get_changeset_fetcher(),
                commit_after_merge,
                10,
            ),
        )
        .unwrap();
        let f = sli.query_reachability(
            ctx.clone(),
            repo.get_changeset_fetcher(),
            commit_after_merge,
            merge_node,
        );
        assert!(run_future(runtime, f).unwrap());

        // perform a query from the merge to the start of branch 1
        let dst_node = string_to_bonsai(
            ctx.clone(),
            &repo,
            "1700524113b1a3b1806560341009684b4378660b",
        );
        // performing this query should index all the nodes inbetween
        let f = sli.query_reachability(
            ctx.clone(),
            repo.get_changeset_fetcher(),
            merge_node,
            dst_node,
        );
        assert!(run_future(runtime, f).unwrap());

        // perform a query from the merge to the start of branch 2
        let dst_node = string_to_bonsai(
            ctx.clone(),
            &repo,
            "1700524113b1a3b1806560341009684b4378660b",
        );
        let f = sli.query_reachability(ctx, repo.get_changeset_fetcher(), merge_node, dst_node);
        assert!(run_future(runtime, f).unwrap());
    }

    fn advance_node_forward(
        ctx: CoreContext,
        changeset_fetcher: Arc<dyn ChangesetFetcher>,
        skip_list_edges: Arc<SkiplistEdgeMapping>,
        (node, gen): (ChangesetId, Generation),
        max_gen: Generation,
    ) -> BoxFuture<NodeFrontier, Error> {
        let initial_frontier = hashmap! {gen => hashset!{node}};
        let initial_frontier = NodeFrontier::new(initial_frontier);
        process_frontier(
            ctx,
            changeset_fetcher,
            skip_list_edges,
            initial_frontier,
            max_gen,
        )
        .boxify()
    }

    fn advance_node_linear(
        runtime: &mut tokio::runtime::Runtime,
        ctx: CoreContext,
        repo: Arc<BlobRepo>,
        sli: SkiplistIndex,
    ) {
        let mut ordered_hashes_oldest_to_newest = vec![
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "0ed509bf086fadcb8a8a5384dc3b550729b0fc17",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "cb15ca4a43a59acff5388cea9648c162afde8372",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "d0a361e9022d226ae52f689667bd7d212a19cfe0",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "607314ef579bd2407752361ba1b0c1729d08b281",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "3e0e761030db6e479a7fb58b12881883f9f8c63f",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536",
            ),
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
                    sli.skip_list_edges.clone(),
                    (node, Generation::new(gen as u64 + 1)),
                    Generation::new(gen_earlier as u64 + 1),
                );
                assert_eq!(
                    run_future(runtime, f).unwrap(),
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
                    sli.skip_list_edges.clone(),
                    (node, Generation::new(gen as u64 + 1)),
                    Generation::new(gen_later as u64 + 1),
                );
                assert_eq!(
                    run_future(runtime, f).unwrap(),
                    NodeFrontier::new(expected_frontier_map)
                );
            }
        }
    }

    fn advance_node_uneven_merge(
        runtime: &mut tokio::runtime::Runtime,
        ctx: CoreContext,
        repo: Arc<BlobRepo>,
        sli: SkiplistIndex,
    ) {
        let root_node = string_to_bonsai(
            ctx.clone(),
            &repo,
            "15c40d0abc36d47fb51c8eaec51ac7aad31f669c",
        );

        // order is oldest to newest
        let branch_1 = vec![
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "3cda5c78aa35f0f5b09780d971197b51cad4613a",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "1d8a907f7b4bf50c6a09c16361e2205047ecc5e5",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "16839021e338500b3cf7c9b871c8a07351697d68",
            ),
        ];

        // order is oldest to newest
        let branch_2 = vec![
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "d7542c9db7f4c77dab4b315edd328edf1514952f",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "b65231269f651cfe784fd1d97ef02a049a37b8a0",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "4f7f3fd428bec1a48f9314414b063c706d9c1aed",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "795b8133cf375f6d68d27c6c23db24cd5d0cd00f",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "bc7b4d0f858c19e2474b03e442b8495fd7aeef33",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "fc2cef43395ff3a7b28159007f63d6529d2f41ca",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "5d43888a3c972fe68c224f93d41b30e9f888df7c",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "264f01429683b3dd8042cb3979e8bf37007118bc",
            ),
        ];
        let merge_node = string_to_bonsai(
            ctx.clone(),
            &repo,
            "7221fa26c85f147db37c2b5f4dbcd5fe52e7645b",
        );

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
                sli.skip_list_edges.clone(),
                (merge_node, Generation::new(10)),
                frontier_generation,
            );
            assert_eq!(
                run_future(runtime, f).unwrap(),
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
                sli.skip_list_edges.clone(),
                (merge_node, Generation::new(10)),
                frontier_generation,
            );
            assert_eq!(
                run_future(runtime, f).unwrap(),
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
            sli.skip_list_edges.clone(),
            (merge_node, Generation::new(10)),
            Generation::new(1),
        );
        assert_eq!(
            run_future(runtime, f).unwrap(),
            NodeFrontier::new(expected_root_frontier_map),
        );
    }

    fn advance_node_on_partial_index(
        runtime: &mut tokio::runtime::Runtime,
        ctx: CoreContext,
        repo: Arc<BlobRepo>,
        sli: SkiplistIndex,
    ) {
        let root_node = string_to_bonsai(
            ctx.clone(),
            &repo,
            "15c40d0abc36d47fb51c8eaec51ac7aad31f669c",
        );

        // order is oldest to newest
        let branch_1 = vec![
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "3cda5c78aa35f0f5b09780d971197b51cad4613a",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "1d8a907f7b4bf50c6a09c16361e2205047ecc5e5",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "16839021e338500b3cf7c9b871c8a07351697d68",
            ),
        ];

        // order is oldest to newest
        let branch_2 = vec![
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "d7542c9db7f4c77dab4b315edd328edf1514952f",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "b65231269f651cfe784fd1d97ef02a049a37b8a0",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "4f7f3fd428bec1a48f9314414b063c706d9c1aed",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "795b8133cf375f6d68d27c6c23db24cd5d0cd00f",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "bc7b4d0f858c19e2474b03e442b8495fd7aeef33",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "fc2cef43395ff3a7b28159007f63d6529d2f41ca",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "5d43888a3c972fe68c224f93d41b30e9f888df7c",
            ),
            string_to_bonsai(
                ctx.clone(),
                &repo,
                "264f01429683b3dd8042cb3979e8bf37007118bc",
            ),
        ];

        let merge_node = string_to_bonsai(
            ctx.clone(),
            &repo,
            "7221fa26c85f147db37c2b5f4dbcd5fe52e7645b",
        );

        // This test partially indexes the top few of the graph.
        // Then it does a query that traverses from indexed to unindexed nodes.
        sli.add_node(ctx.clone(), repo.get_changeset_fetcher(), merge_node, 2);

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
            sli.skip_list_edges.clone(),
            (merge_node, Generation::new(10)),
            Generation::new(1),
        );
        assert_eq!(
            run_future(runtime, f).unwrap(),
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
                sli.skip_list_edges.clone(),
                (merge_node, Generation::new(10)),
                frontier_generation,
            );
            assert_eq!(
                run_future(runtime, f).unwrap(),
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
                sli.skip_list_edges.clone(),
                (merge_node, Generation::new(10)),
                frontier_generation,
            );
            assert_eq!(
                run_future(runtime, f).unwrap(),
                NodeFrontier::new(expected_frontier_map)
            );
        }
    }

    fn simul_node_advance_on_wide_branch(
        runtime: &mut tokio::runtime::Runtime,
        ctx: CoreContext,
        repo: Arc<BlobRepo>,
        sli: SkiplistIndex,
    ) {
        let root_node = string_to_bonsai(
            ctx.clone(),
            &repo,
            "ecba698fee57eeeef88ac3dcc3b623ede4af47bd",
        );

        let _b1 = string_to_bonsai(
            ctx.clone(),
            &repo,
            "9e8521affb7f9d10e9551a99c526e69909042b20",
        );
        let _b2 = string_to_bonsai(
            ctx.clone(),
            &repo,
            "4685e9e62e4885d477ead6964a7600c750e39b03",
        );
        let b1_1 = string_to_bonsai(
            ctx.clone(),
            &repo,
            "b6a8169454af58b4b72b3665f9aa0d25529755ff",
        );
        let b1_2 = string_to_bonsai(
            ctx.clone(),
            &repo,
            "c27ef5b7f15e9930e5b93b1f32cc2108a2aabe12",
        );
        let b2_1 = string_to_bonsai(
            ctx.clone(),
            &repo,
            "04decbb0d1a65789728250ddea2fe8d00248e01c",
        );
        let b2_2 = string_to_bonsai(
            ctx.clone(),
            &repo,
            "49f53ab171171b3180e125b918bd1cf0af7e5449",
        );

        let advance_to_root_futures =
            vec![b1_1, b1_2, b2_1, b2_2]
                .into_iter()
                .map(move |branch_tip| {
                    advance_node_forward(
                        ctx.clone(),
                        repo.get_changeset_fetcher(),
                        sli.skip_list_edges.clone(),
                        (branch_tip, Generation::new(3)),
                        Generation::new(1),
                    )
                });
        let advanced_frontiers = join_all(advance_to_root_futures);
        let advanced_frontiers = run_future(runtime, advanced_frontiers).unwrap();
        let mut expected_root_frontier_map = HashMap::new();
        expected_root_frontier_map
            .insert(Generation::new(1), vec![root_node].into_iter().collect());

        let expected_root_frontier = NodeFrontier::new(expected_root_frontier_map);
        for frontier in advanced_frontiers.into_iter() {
            assert_eq!(frontier, expected_root_frontier);
        }
    }

    fn process_frontier_on_wide_branch(
        runtime: &mut tokio::runtime::Runtime,
        ctx: CoreContext,
        repo: Arc<BlobRepo>,
        sli: SkiplistIndex,
    ) {
        let root_node = string_to_bonsai(
            ctx.clone(),
            &repo,
            "ecba698fee57eeeef88ac3dcc3b623ede4af47bd",
        );

        let b1 = string_to_bonsai(
            ctx.clone(),
            &repo,
            "9e8521affb7f9d10e9551a99c526e69909042b20",
        );
        let b2 = string_to_bonsai(
            ctx.clone(),
            &repo,
            "4685e9e62e4885d477ead6964a7600c750e39b03",
        );
        let b1_1 = string_to_bonsai(
            ctx.clone(),
            &repo,
            "b6a8169454af58b4b72b3665f9aa0d25529755ff",
        );
        let b1_2 = string_to_bonsai(
            ctx.clone(),
            &repo,
            "c27ef5b7f15e9930e5b93b1f32cc2108a2aabe12",
        );
        let b2_1 = string_to_bonsai(
            ctx.clone(),
            &repo,
            "04decbb0d1a65789728250ddea2fe8d00248e01c",
        );
        let b2_2 = string_to_bonsai(
            ctx.clone(),
            &repo,
            "49f53ab171171b3180e125b918bd1cf0af7e5449",
        );

        let mut starting_frontier_map = HashMap::new();
        starting_frontier_map.insert(
            Generation::new(3),
            vec![b1_1, b1_2, b2_1, b2_2].into_iter().collect(),
        );

        let mut expected_gen_2_frontier_map = HashMap::new();
        expected_gen_2_frontier_map.insert(Generation::new(2), vec![b1, b2].into_iter().collect());
        let f = process_frontier(
            ctx.clone(),
            repo.get_changeset_fetcher(),
            sli.skip_list_edges.clone(),
            NodeFrontier::new(starting_frontier_map.clone()),
            Generation::new(2),
        );
        assert_eq!(
            run_future(runtime, f).unwrap(),
            NodeFrontier::new(expected_gen_2_frontier_map)
        );

        let mut expected_root_frontier_map = HashMap::new();
        expected_root_frontier_map
            .insert(Generation::new(1), vec![root_node].into_iter().collect());
        let f = process_frontier(
            ctx,
            repo.get_changeset_fetcher(),
            sli.skip_list_edges.clone(),
            NodeFrontier::new(starting_frontier_map),
            Generation::new(1),
        );
        assert_eq!(
            run_future(runtime, f).unwrap(),
            NodeFrontier::new(expected_root_frontier_map)
        );
    }

    fn test_is_ancestor(
        runtime: &mut tokio::runtime::Runtime,
        ctx: CoreContext,
        repo: Arc<BlobRepo>,
        sli: SkiplistIndex,
    ) {
        let f = repo
            .get_bonsai_bookmark(ctx.clone(), &BookmarkName::new("master").unwrap())
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
                for anc in cs_ancestor_map.keys() {
                    for desc in cs_ancestor_map.keys() {
                        cloned!(ctx, repo, anc, desc);
                        let expected_and_params = Ok((
                            cs_ancestor_map.get(&desc).unwrap().contains(&anc),
                            (anc, desc),
                        ))
                        .into_future();
                        let actual = sli.is_ancestor(ctx, repo.get_changeset_fetcher(), anc, desc);
                        res.push(actual.join(expected_and_params))
                    }
                }
                join_all(res).map(|res| {
                    res.into_iter()
                        .all(|(actual, (expected, _))| actual == expected)
                })
            });

        assert!(run_future(runtime, f).unwrap());
    }

    fn test_is_ancestor_merge_uneven(
        runtime: &mut tokio::runtime::Runtime,
        ctx: CoreContext,
        repo: Arc<BlobRepo>,
        sli: SkiplistIndex,
    ) {
        test_is_ancestor(runtime, ctx, repo, sli)
    }

    fn test_is_ancestor_unshared_merge_even(
        runtime: &mut tokio::runtime::Runtime,
        ctx: CoreContext,
        repo: Arc<BlobRepo>,
        sli: SkiplistIndex,
    ) {
        test_is_ancestor(runtime, ctx, repo, sli)
    }

    skiplist_test!(query_reachability_hint_on_self_is_true, linear);
    skiplist_test!(query_reachability_to_higher_gen_is_false, linear);
    skiplist_test!(query_from_indexed_merge_node, unshared_merge_even);
    skiplist_test!(advance_node_linear, linear);
    skiplist_test!(advance_node_uneven_merge, merge_uneven);
    skiplist_test!(advance_node_on_partial_index, merge_uneven);
    skiplist_test!(simul_node_advance_on_wide_branch, branch_wide);
    skiplist_test!(process_frontier_on_wide_branch, branch_wide);
    skiplist_test!(test_is_ancestor_merge_uneven, merge_uneven);
    skiplist_test!(test_is_ancestor_unshared_merge_even, unshared_merge_even);
}
