// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use std::sync::Arc;

use blobrepo::BlobRepo;
use context::CoreContext;
use fixtures::{branch_wide, linear, merge_uneven};
use futures::future::Future;

#[cfg(test)]
use common::fetch_generation_and_join;
use mercurial_types::{HgChangesetId, HgNodeHash};
use mononoke_types::ChangesetId;
use reachabilityindex::ReachabilityIndex;

pub fn string_to_nodehash(hash: &'static str) -> HgNodeHash {
    HgNodeHash::from_static_str(hash).expect("Can't turn string to HgNodeHash")
}

pub fn string_to_bonsai(ctx: CoreContext, repo: &Arc<BlobRepo>, s: &'static str) -> ChangesetId {
    let node = string_to_nodehash(s);
    repo.get_bonsai_from_hg(ctx, HgChangesetId::new(node))
        .wait()
        .unwrap()
        .unwrap()
}

pub fn test_linear_reachability<T: ReachabilityIndex + 'static>(index_creator: fn() -> T) {
    async_unit::tokio_unit_test(move || {
        let ctx = CoreContext::test_mock();
        let repo = Arc::new(linear::getrepo());
        let index = index_creator();
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

        for i in 0..ordered_hashes.len() {
            for j in i..ordered_hashes.len() {
                let src = ordered_hashes.get(i).unwrap();
                let dst = ordered_hashes.get(j).unwrap();
                let future_result_src_to_dst =
                    index.query_reachability(ctx.clone(), repo.get_changeset_fetcher(), *src, *dst);
                assert!(future_result_src_to_dst.wait().unwrap());
                let future_result_dst_to_src =
                    index.query_reachability(ctx.clone(), repo.get_changeset_fetcher(), *dst, *src);
                assert_eq!(future_result_dst_to_src.wait().unwrap(), src == dst);
            }
        }
    });
}

pub fn test_merge_uneven_reachability<T: ReachabilityIndex + 'static>(index_creator: fn() -> T) {
    async_unit::tokio_unit_test(move || {
        let ctx = CoreContext::test_mock();
        let repo = Arc::new(merge_uneven::getrepo());
        let index = index_creator();
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

        let _merge_node = string_to_bonsai(
            ctx.clone(),
            &repo,
            "7221fa26c85f147db37c2b5f4dbcd5fe52e7645b",
        );

        for left_node in branch_1.into_iter() {
            for right_node in branch_2.iter() {
                assert!(index
                    .query_reachability(
                        ctx.clone(),
                        repo.get_changeset_fetcher(),
                        left_node,
                        root_node
                    )
                    .wait()
                    .unwrap());
                assert!(index
                    .query_reachability(
                        ctx.clone(),
                        repo.get_changeset_fetcher(),
                        *right_node,
                        root_node
                    )
                    .wait()
                    .unwrap());
                assert!(!index
                    .query_reachability(
                        ctx.clone(),
                        repo.get_changeset_fetcher(),
                        root_node,
                        left_node
                    )
                    .wait()
                    .unwrap());
                assert!(!index
                    .query_reachability(
                        ctx.clone(),
                        repo.get_changeset_fetcher(),
                        root_node,
                        *right_node
                    )
                    .wait()
                    .unwrap());
            }
        }
    });
}

pub fn test_branch_wide_reachability<T: ReachabilityIndex + 'static>(index_creator: fn() -> T) {
    async_unit::tokio_unit_test(move || {
        let ctx = CoreContext::test_mock();
        // this repo has no merges but many branches
        let repo = Arc::new(branch_wide::getrepo());
        let index = index_creator();
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

        // all nodes can reach the root
        for above_root in vec![b1, b2, b1_1, b1_2, b2_1, b2_2].iter() {
            assert!(index
                .query_reachability(
                    ctx.clone(),
                    repo.get_changeset_fetcher(),
                    *above_root,
                    root_node
                )
                .wait()
                .unwrap());
            assert!(!index
                .query_reachability(
                    ctx.clone(),
                    repo.get_changeset_fetcher(),
                    root_node,
                    *above_root
                )
                .wait()
                .unwrap());
        }

        // nodes in different branches cant reach each other
        for b1_node in vec![b1, b1_1, b1_2].iter() {
            for b2_node in vec![b2, b2_1, b2_2].iter() {
                assert!(!index
                    .query_reachability(
                        ctx.clone(),
                        repo.get_changeset_fetcher(),
                        *b1_node,
                        *b2_node
                    )
                    .wait()
                    .unwrap());
                assert!(!index
                    .query_reachability(
                        ctx.clone(),
                        repo.get_changeset_fetcher(),
                        *b2_node,
                        *b1_node
                    )
                    .wait()
                    .unwrap());
            }
        }

        // branch nodes can reach their common root but not each other
        // - branch 1
        assert!(index
            .query_reachability(ctx.clone(), repo.get_changeset_fetcher(), b1_1, b1)
            .wait()
            .unwrap());
        assert!(index
            .query_reachability(ctx.clone(), repo.get_changeset_fetcher(), b1_2, b1)
            .wait()
            .unwrap());
        assert!(!index
            .query_reachability(ctx.clone(), repo.get_changeset_fetcher(), b1_1, b1_2)
            .wait()
            .unwrap());
        assert!(!index
            .query_reachability(ctx.clone(), repo.get_changeset_fetcher(), b1_2, b1_1)
            .wait()
            .unwrap());

        // - branch 2
        assert!(index
            .query_reachability(ctx.clone(), repo.get_changeset_fetcher(), b2_1, b2)
            .wait()
            .unwrap());
        assert!(index
            .query_reachability(ctx.clone(), repo.get_changeset_fetcher(), b2_2, b2)
            .wait()
            .unwrap());
        assert!(!index
            .query_reachability(ctx.clone(), repo.get_changeset_fetcher(), b2_1, b2_2)
            .wait()
            .unwrap());
        assert!(!index
            .query_reachability(ctx.clone(), repo.get_changeset_fetcher(), b2_2, b2_1)
            .wait()
            .unwrap());
    });
}

#[cfg(test)]
mod test {
    use super::*;

    use std::sync::Arc;

    use async_unit;
    use context::CoreContext;
    use fixtures::linear;
    use futures::Future;
    use mononoke_types::Generation;

    #[test]
    fn test_helpers() {
        async_unit::tokio_unit_test(move || {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(linear::getrepo());
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

            for (i, node) in ordered_hashes_oldest_to_newest.into_iter().enumerate() {
                assert_eq!(
                    fetch_generation_and_join(ctx.clone(), repo.get_changeset_fetcher(), node)
                        .wait()
                        .unwrap(),
                    (node, Generation::new(i as u64 + 1))
                );
            }
        });
    }
}
