// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashSet;
use std::sync::Arc;

use failure::{err_msg, Error};
use futures::IntoFuture;
use futures::future::{loop_fn, ok, Future, Loop};
use futures::stream::{iter_ok, Stream};
use futures_ext::FutureExt;

use blobrepo::BlobRepo;
use mercurial_types::HgNodeHash;
use mercurial_types::nodehash::HgChangesetId;
use mononoke_types::Generation;

use index::ReachabilityIndex;

pub struct GenerationNumberBFS {
    repo: Arc<BlobRepo>,
}

impl GenerationNumberBFS {
    pub fn new(repo: Arc<BlobRepo>) -> Self {
        GenerationNumberBFS { repo }
    }
}

// Take ownership of two sets, the current 'layer' of the bfs, and all nodes seen until then.
// Produce a future which does the following computation:
// - add all nodes in the current layer to the seen set
// - get the set of parents of nodes in the current layer
// - filter out previously seen nodes from the parents
// - filter out nodes whose generation number is too small
// - return the parents as the next bfs layer, and the updated seen as the new seen set
fn process_bfs_layer(
    repo: Arc<BlobRepo>,
    curr_layer: HashSet<HgNodeHash>,
    mut curr_seen: HashSet<HgNodeHash>,
    dst_gen: Generation,
) -> Box<Future<Item = (HashSet<HgNodeHash>, HashSet<HgNodeHash>), Error = Error> + Send> {
    let new_repo_changesets = repo.clone();
    let new_repo_gennums = repo.clone();
    for next_node in curr_layer.iter() {
        curr_seen.insert(next_node.clone());
    }

    iter_ok::<_, Error>(curr_layer)
        .and_then(move |hash| new_repo_changesets.get_changeset_parents(&HgChangesetId::new(hash)))
        .map(|parents| iter_ok::<_, Error>(parents.into_iter()))
        .flatten()
        .and_then(move |node_cs| {
            new_repo_gennums
                .get_generation_number(&node_cs)
                .and_then(move |genopt| {
                    genopt.ok_or_else(|| err_msg(format!("{} not found", node_cs)))
                })
                .map(move |gen_id| (*node_cs.as_nodehash(), gen_id))
        })
        .collect()
        .map(move |flattened_node_generation_pairs| {
            let mut next_layer = HashSet::new();
            for (parent_hash, parent_gen) in flattened_node_generation_pairs {
                if !curr_seen.contains(&parent_hash) && parent_gen >= dst_gen {
                    next_layer.insert(parent_hash);
                }
            }
            (next_layer, curr_seen)
        })
        .boxify()
}

impl ReachabilityIndex for GenerationNumberBFS {
    fn query_reachability(
        &mut self,
        repo: Arc<BlobRepo>,
        src: HgNodeHash,
        dst: HgNodeHash,
    ) -> Box<Future<Item = bool, Error = Error>> {
        let start_bfs_layer: HashSet<_> = vec![src].into_iter().collect();
        let start_seen: HashSet<_> = HashSet::new();
        repo.get_generation_number(&HgChangesetId::new(dst.clone()))
            .and_then(move |dst_gen_opt: Option<_>| {
                dst_gen_opt.ok_or_else(|| err_msg(format!("{} not found", dst)))
            })
            .and_then(move |dst_gen| {
                loop_fn(
                    (start_bfs_layer, start_seen),
                    move |(curr_layer, curr_seen)| {
                        if curr_layer.contains(&dst) {
                            ok(Loop::Break(true)).boxify()
                        } else if curr_layer.is_empty() {
                            ok(Loop::Break(false)).boxify()
                        } else {
                            process_bfs_layer(repo.clone(), curr_layer, curr_seen, dst_gen)
                                .map(move |(next_layer, next_seen)| {
                                    Loop::Continue((next_layer, next_seen))
                                })
                                .boxify()
                        }
                    },
                )
            })
            .boxify()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use async_unit;
    use branch_wide;
    use linear;
    use merge_uneven;

    fn string_to_nodehash(hash: &'static str) -> HgNodeHash {
        HgNodeHash::from_static_str(hash).expect("Can't turn string to HgNodeHash")
    }

    #[test]
    fn linear_reachability() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));
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
            let mut bfs = GenerationNumberBFS::new(repo.clone());

            for i in 0..ordered_hashes.len() {
                for j in i..ordered_hashes.len() {
                    let src = ordered_hashes.get(i).unwrap();
                    let dst = ordered_hashes.get(j).unwrap();
                    let future_result_src_to_dst = bfs.query_reachability(repo.clone(), *src, *dst);
                    assert!(future_result_src_to_dst.wait().unwrap());
                    let future_result_dst_to_src = bfs.query_reachability(repo.clone(), *dst, *src);
                    assert_eq!(future_result_dst_to_src.wait().unwrap(), src == dst);
                }
            }
        });
    }

    #[test]
    fn merge_uneven_reachability() {
        async_unit::tokio_unit_test(|| {
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

            let mut bfs = GenerationNumberBFS::new(repo.clone());

            for left_node in branch_1.into_iter() {
                for right_node in branch_2.iter() {
                    assert!(
                        bfs.query_reachability(repo.clone(), left_node, root_node)
                            .wait()
                            .unwrap()
                    );
                    assert!(
                        bfs.query_reachability(repo.clone(), *right_node, root_node)
                            .wait()
                            .unwrap()
                    );
                    assert!(!bfs.query_reachability(repo.clone(), root_node, left_node)
                        .wait()
                        .unwrap());
                    assert!(
                        !bfs.query_reachability(repo.clone(), root_node, *right_node)
                            .wait()
                            .unwrap()
                    );
                }
            }
        });
    }

    #[test]
    fn branch_wide_reachability() {
        async_unit::tokio_unit_test(|| {
            // this repo has no merges but many branches
            let repo = Arc::new(branch_wide::getrepo(None));
            let root_node = string_to_nodehash("ecba698fee57eeeef88ac3dcc3b623ede4af47bd");

            let b1 = string_to_nodehash("9e8521affb7f9d10e9551a99c526e69909042b20");
            let b2 = string_to_nodehash("4685e9e62e4885d477ead6964a7600c750e39b03");
            let b1_1 = string_to_nodehash("b6a8169454af58b4b72b3665f9aa0d25529755ff");
            let b1_2 = string_to_nodehash("c27ef5b7f15e9930e5b93b1f32cc2108a2aabe12");
            let b2_1 = string_to_nodehash("04decbb0d1a65789728250ddea2fe8d00248e01c");
            let b2_2 = string_to_nodehash("49f53ab171171b3180e125b918bd1cf0af7e5449");
            let mut bfs = GenerationNumberBFS::new(repo.clone());

            // all nodes can reach the root
            for above_root in vec![b1, b2, b1_1, b1_2, b2_1, b2_2].iter() {
                assert!(
                    bfs.query_reachability(repo.clone(), *above_root, root_node)
                        .wait()
                        .unwrap()
                );
                assert!(
                    !bfs.query_reachability(repo.clone(), root_node, *above_root)
                        .wait()
                        .unwrap()
                );
            }

            // nodes in different branches cant reach each other
            for b1_node in vec![b1, b1_1, b1_2].iter() {
                for b2_node in vec![b2, b2_1, b2_2].iter() {
                    assert!(!bfs.query_reachability(repo.clone(), *b1_node, *b2_node)
                        .wait()
                        .unwrap());
                    assert!(!bfs.query_reachability(repo.clone(), *b2_node, *b1_node)
                        .wait()
                        .unwrap());
                }
            }

            // branch nodes can reach their common root but not each other
            // - branch 1
            assert!(
                bfs.query_reachability(repo.clone(), b1_1, b1)
                    .wait()
                    .unwrap()
            );
            assert!(
                bfs.query_reachability(repo.clone(), b1_2, b1)
                    .wait()
                    .unwrap()
            );
            assert!(!bfs.query_reachability(repo.clone(), b1_1, b1_2)
                .wait()
                .unwrap());
            assert!(!bfs.query_reachability(repo.clone(), b1_2, b1_1)
                .wait()
                .unwrap());

            // - branch 2
            assert!(
                bfs.query_reachability(repo.clone(), b2_1, b2)
                    .wait()
                    .unwrap()
            );
            assert!(
                bfs.query_reachability(repo.clone(), b2_2, b2)
                    .wait()
                    .unwrap()
            );
            assert!(!bfs.query_reachability(repo.clone(), b2_1, b2_2)
                .wait()
                .unwrap());
            assert!(!bfs.query_reachability(repo.clone(), b2_2, b2_1)
                .wait()
                .unwrap());
        });
    }
}
