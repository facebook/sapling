// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::ops::Deref;
use std::sync::Arc;

use chashmap::CHashMap;
use failure::Error;
use futures::future::{join_all, ok, Future};
use futures_ext::{BoxFuture, FutureExt};

use blobrepo::BlobRepo;
use mercurial_types::HgNodeHash;
use mononoke_types::Generation;

use helpers::{changeset_to_nodehashes_with_generation_numbers, get_parents_from_nodehash};

const DEFAULT_EDGE_COUNT: u32 = 10;

// Each indexed node fits into one of two categories:
// - It has skiplist edges
// - It only has edges to its parents.
enum SkiplistNodeType {
    // A list of skip edges which keep doubling
    // in distance from their root node.
    // The ith skip edge is at most 2^i commits away.
    SkipEdges(Vec<(HgNodeHash, Generation)>),
    ParentEdges(Vec<(HgNodeHash, Generation)>),
}

struct SkiplistEdgeMapping {
    pub mapping: CHashMap<HgNodeHash, SkiplistNodeType>,
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
    start_node: (HgNodeHash, Generation),
    skip_edge_mapping: Arc<SkiplistEdgeMapping>,
) -> Vec<(HgNodeHash, Generation)> {
    let mut curr = start_node;

    let max_skip_edge_count = skip_edge_mapping.skip_edges_per_node as usize;
    let mut skip_edges = vec![curr];
    let mut i: usize = 0;

    while let Some(read_locked_entry) = skip_edge_mapping.mapping.get(&curr.0) {
        if let SkiplistNodeType::SkipEdges(edges) = read_locked_entry.deref() {
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

/// From a starting node, index all nodes that are reachable within a given distance.
/// If a previously indexed node is reached, indexing will stop there.
fn lazy_index_node(
    repo: Arc<BlobRepo>,
    skip_edge_mapping: Arc<SkiplistEdgeMapping>,
    node: HgNodeHash,
    max_depth: u64,
) -> BoxFuture<(), Error> {
    // if this node is indexed or we've passed the max depth, return
    if max_depth == 0 || skip_edge_mapping.mapping.contains_key(&node) {
        ok(()).boxify()
    } else {
        get_parents_from_nodehash(repo.clone(), node)
            // for each parent of this node:
            .and_then({cloned!(skip_edge_mapping, repo); move |parents| {
                join_all(parents.clone().into_iter()
                    // call the lazy index on the parents in parallel
                    .map({cloned!(repo, skip_edge_mapping); move |parent| {
                        lazy_index_node(
                            repo.clone(),
                            skip_edge_mapping.clone(),
                            *parent.as_nodehash(),
                            max_depth - 1,
                        )
                    }})
                )
                    .map(move |_| parents)
                    .and_then(move |all_parents| {
                        changeset_to_nodehashes_with_generation_numbers(
                            repo.clone(),
                            all_parents,
                        )
                    })
            }})
            .and_then(move |parent_gen_pairs| {
                // determine what kind of skip edges to add for this node
                if parent_gen_pairs.len() != 1 {
                    // Merge node or parentless node
                    // Reflect this in the index
                        ok(skip_edge_mapping.clone()
                            .mapping
                            .insert(node, SkiplistNodeType::ParentEdges(parent_gen_pairs)))
                        .and_then(|_| ok(()))
                        .left_future()
                } else {
                    // Single parent node
                    // Compute skip edges assuming a reasonable number of parents are indexed.
                    // Even if this reaches a non indexed node during,
                    // indexing correctness isn't affected.
                    let unique_parent_gen_pair = parent_gen_pairs.get(0).unwrap().clone();
                            ok(compute_skip_edges(
                                unique_parent_gen_pair,
                                skip_edge_mapping.clone(),
                                ))
                            .map(move |new_edges| skip_edge_mapping.clone()
                                .mapping
                                .insert(node, SkiplistNodeType::SkipEdges(new_edges)))
                            .and_then(|_| ok(()))
                            .right_future()
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

    pub fn with_skip_edge_count(self, skip_edges_per_node: u32) -> Self {
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
        repo: Arc<BlobRepo>,
        node: HgNodeHash,
        max_index_depth: u64,
    ) -> BoxFuture<(), Error> {
        lazy_index_node(repo, self.skip_list_edges.clone(), node, max_index_depth)
    }

    /// get skiplist edges originating from a particular node hash
    /// returns Some(edges) if this node was indexed with skip edges
    /// returns None if this node was unindexed, or was indexed with parent edges only.
    pub fn get_skip_edges(&self, node: HgNodeHash) -> Option<Vec<(HgNodeHash, Generation)>> {
        if let Some(read_guard) = self.skip_list_edges.mapping.get(&node) {
            if let SkiplistNodeType::SkipEdges(edges) = read_guard.deref() {
                Some(edges.clone())
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn is_node_indexed(&self, node: HgNodeHash) -> bool {
        self.skip_list_edges.mapping.contains_key(&node)
    }

    pub fn indexed_node_count(&self) -> usize {
        self.skip_list_edges.mapping.len()
    }
}

#[cfg(test)]
mod test {
    use async_unit;
    use chashmap::CHashMap;
    use std::sync::Arc;

    use futures::stream::Stream;
    use futures::stream::iter_ok;

    use super::*;
    use branch_wide;
    use linear;
    use merge_uneven;
    use tests::string_to_nodehash;

    #[test]
    fn simple_init() {
        async_unit::tokio_unit_test(|| {
            let sli = SkiplistIndex::new();
            assert_eq!(sli.skip_edge_count(), DEFAULT_EDGE_COUNT);

            let sli_with_20 = SkiplistIndex::new().with_skip_edge_count(20);
            assert_eq!(sli_with_20.skip_edge_count(), 20);
        });
    }

    #[test]
    fn arc_chash_is_sync_and_send() {
        fn is_sync<T: Sync>() {}
        fn is_send<T: Send>() {}

        is_sync::<Arc<CHashMap<HgNodeHash, SkiplistNodeType>>>();
        is_send::<Arc<CHashMap<HgNodeHash, SkiplistNodeType>>>();
    }

    #[test]
    fn test_add_node() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));
            let sli = SkiplistIndex::new();
            let master_node = string_to_nodehash("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157");
            sli.add_node(repo, master_node, 100).wait().unwrap();
            let ordered_hashes = vec![
                string_to_nodehash("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157"),
                string_to_nodehash("0ed509bf086fadcb8a8a5384dc3b550729b0fc17"),
                string_to_nodehash("eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b"),
                string_to_nodehash("cb15ca4a43a59acff5388cea9648c162afde8372"),
                string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
                string_to_nodehash("607314ef579bd2407752361ba1b0c1729d08b281"),
                string_to_nodehash("3e0e761030db6e479a7fb58b12881883f9f8c63f"),
                string_to_nodehash("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536"),
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
            let repo = Arc::new(linear::getrepo(None));
            let sli = SkiplistIndex::new();
            let master_node = string_to_nodehash("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157");
            sli.add_node(repo, master_node, 100).wait().unwrap();
            let ordered_hashes = vec![
                string_to_nodehash("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157"),
                string_to_nodehash("0ed509bf086fadcb8a8a5384dc3b550729b0fc17"),
                string_to_nodehash("eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b"),
                string_to_nodehash("cb15ca4a43a59acff5388cea9648c162afde8372"),
                string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
                string_to_nodehash("607314ef579bd2407752361ba1b0c1729d08b281"),
                string_to_nodehash("3e0e761030db6e479a7fb58b12881883f9f8c63f"),
                string_to_nodehash("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536"),
            ];
            assert_eq!(sli.indexed_node_count(), ordered_hashes.len());
            for node in ordered_hashes.into_iter() {
                assert!(sli.is_node_indexed(node));
                if node != string_to_nodehash("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536") {
                    let skip_edges: Vec<HgNodeHash> = sli.get_skip_edges(node)
                        .unwrap()
                        .into_iter()
                        .map(|(node, _)| node)
                        .collect();
                    assert!(skip_edges.contains(&string_to_nodehash(
                        "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536"
                    )));
                }
            }
        });
    }

    #[test]
    fn test_skip_edges_progress_powers_of_2() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));
            let sli = SkiplistIndex::new();
            let master_node = string_to_nodehash("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157");
            sli.add_node(repo, master_node, 100).wait().unwrap();
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

            let skip_edges: Vec<HgNodeHash> = sli.get_skip_edges(master_node)
                .unwrap()
                .into_iter()
                .map(|(node, _)| node)
                .collect();
            let expected_hashes = vec![
                string_to_nodehash("0ed509bf086fadcb8a8a5384dc3b550729b0fc17"),
                string_to_nodehash("eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b"),
                string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
                string_to_nodehash("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536"),
            ];
            assert_eq!(skip_edges, expected_hashes);
        });
    }

    #[test]
    fn test_skip_edges_reach_end_in_merge() {
        async_unit::tokio_unit_test(move || {
            let repo = Arc::new(merge_uneven::getrepo(None));
            let root_node = string_to_nodehash("15c40d0abc36d47fb51c8eaec51ac7aad31f669c");

            // order is oldest to newest
            let branch_1 = vec![
                string_to_nodehash("3cda5c78aa35f0f5b09780d971197b51cad4613a"),
                string_to_nodehash("1d8a907f7b4bf50c6a09c16361e2205047ecc5e5"),
                string_to_nodehash("16839021e338500b3cf7c9b871c8a07351697d68"),
            ];

            // order is oldest to newest
            let branch_2 = vec![
                string_to_nodehash("d7542c9db7f4c77dab4b315edd328edf1514952f"),
                string_to_nodehash("b65231269f651cfe784fd1d97ef02a049a37b8a0"),
                string_to_nodehash("4f7f3fd428bec1a48f9314414b063c706d9c1aed"),
                string_to_nodehash("795b8133cf375f6d68d27c6c23db24cd5d0cd00f"),
                string_to_nodehash("bc7b4d0f858c19e2474b03e442b8495fd7aeef33"),
                string_to_nodehash("fc2cef43395ff3a7b28159007f63d6529d2f41ca"),
                string_to_nodehash("5d43888a3c972fe68c224f93d41b30e9f888df7c"),
                string_to_nodehash("264f01429683b3dd8042cb3979e8bf37007118bc"),
            ];

            let merge_node = string_to_nodehash("75742e6fc286a359b39a89fdfa437cc7e2a0e1ce");
            let sli = SkiplistIndex::new();
            sli.add_node(repo, merge_node, 100).wait().unwrap();
            for node in branch_1.into_iter() {
                let skip_edges: Vec<HgNodeHash> = sli.get_skip_edges(node)
                    .unwrap()
                    .into_iter()
                    .map(|(node, _)| node)
                    .collect();
                assert!(skip_edges.contains(&root_node));
            }
            for node in branch_2.into_iter() {
                let skip_edges: Vec<HgNodeHash> = sli.get_skip_edges(node)
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
            let repo = Arc::new(merge_uneven::getrepo(None));
            let root_node = string_to_nodehash("15c40d0abc36d47fb51c8eaec51ac7aad31f669c");

            // order is oldest to newest
            let branch_1 = vec![
                string_to_nodehash("3cda5c78aa35f0f5b09780d971197b51cad4613a"),
                string_to_nodehash("1d8a907f7b4bf50c6a09c16361e2205047ecc5e5"),
                string_to_nodehash("16839021e338500b3cf7c9b871c8a07351697d68"),
            ];

            let branch_1_head = string_to_nodehash("16839021e338500b3cf7c9b871c8a07351697d68");

            // order is oldest to newest
            let branch_2 = vec![
                string_to_nodehash("d7542c9db7f4c77dab4b315edd328edf1514952f"),
                string_to_nodehash("b65231269f651cfe784fd1d97ef02a049a37b8a0"),
                string_to_nodehash("4f7f3fd428bec1a48f9314414b063c706d9c1aed"),
                string_to_nodehash("795b8133cf375f6d68d27c6c23db24cd5d0cd00f"),
                string_to_nodehash("bc7b4d0f858c19e2474b03e442b8495fd7aeef33"),
                string_to_nodehash("fc2cef43395ff3a7b28159007f63d6529d2f41ca"),
                string_to_nodehash("5d43888a3c972fe68c224f93d41b30e9f888df7c"),
                string_to_nodehash("264f01429683b3dd8042cb3979e8bf37007118bc"),
            ];
            let branch_2_head = string_to_nodehash("264f01429683b3dd8042cb3979e8bf37007118bc");

            let _merge_node = string_to_nodehash("75742e6fc286a359b39a89fdfa437cc7e2a0e1ce");
            let sli = SkiplistIndex::new();

            // index just one branch first
            sli.add_node(repo.clone(), branch_1_head, 100)
                .wait()
                .unwrap();
            for node in branch_1.into_iter() {
                let skip_edges: Vec<HgNodeHash> = sli.get_skip_edges(node)
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
            sli.add_node(repo, branch_2_head, 100).wait().unwrap();
            for node in branch_2.into_iter() {
                let skip_edges: Vec<HgNodeHash> = sli.get_skip_edges(node)
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
            // this repo has no merges but many branches
            let repo = Arc::new(branch_wide::getrepo(None));
            let root_node = string_to_nodehash("ecba698fee57eeeef88ac3dcc3b623ede4af47bd");

            let b1 = string_to_nodehash("9e8521affb7f9d10e9551a99c526e69909042b20");
            let b2 = string_to_nodehash("4685e9e62e4885d477ead6964a7600c750e39b03");
            let b1_1 = string_to_nodehash("b6a8169454af58b4b72b3665f9aa0d25529755ff");
            let b1_2 = string_to_nodehash("c27ef5b7f15e9930e5b93b1f32cc2108a2aabe12");
            let b2_1 = string_to_nodehash("04decbb0d1a65789728250ddea2fe8d00248e01c");
            let b2_2 = string_to_nodehash("49f53ab171171b3180e125b918bd1cf0af7e5449");

            let sli = SkiplistIndex::new();
            iter_ok::<_, Error>(vec![b1_1, b1_2, b2_1, b2_2])
                .map(|branch_tip| sli.add_node(repo.clone(), branch_tip, 100))
                .buffered(4)
                .for_each(|_| ok(()))
                .wait()
                .unwrap();
            assert!(sli.is_node_indexed(root_node));
            assert!(sli.is_node_indexed(b1));
            assert!(sli.is_node_indexed(b2));

            for node in vec![b1, b2, b1_1, b1_2, b2_1, b2_2].into_iter() {
                let skip_edges: Vec<HgNodeHash> = sli.get_skip_edges(node)
                    .unwrap()
                    .into_iter()
                    .map(|(node, _)| node)
                    .collect();
                assert!(skip_edges.contains(&root_node));
            }
        });
    }
}
