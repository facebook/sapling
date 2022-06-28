/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use blobrepo::BlobRepo;
use context::CoreContext;
use fbinit::FacebookInit;
use fixtures::BranchWide;
use fixtures::Linear;
use fixtures::MergeUneven;
use fixtures::TestRepoFixture;

#[cfg(test)]
use common::fetch_generation;
use mercurial_types::HgChangesetId;
use mercurial_types::HgNodeHash;
use mononoke_types::ChangesetId;
use reachabilityindex::ReachabilityIndex;

pub fn string_to_nodehash(hash: &'static str) -> HgNodeHash {
    HgNodeHash::from_static_str(hash).expect("Can't turn string to HgNodeHash")
}

pub async fn string_to_bonsai<'a>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    s: &'static str,
) -> ChangesetId {
    let node = string_to_nodehash(s);
    repo.bonsai_hg_mapping()
        .get_bonsai_from_hg(ctx, HgChangesetId::new(node))
        .await
        .unwrap()
        .unwrap()
}

pub async fn test_linear_reachability<T: ReachabilityIndex + 'static>(
    fb: FacebookInit,
    index_creator: fn() -> T,
) {
    let ctx = CoreContext::test_mock(fb);
    let repo = Arc::new(Linear::getrepo(fb).await);
    let index = index_creator();
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

    for i in 0..ordered_hashes.len() {
        for j in i..ordered_hashes.len() {
            let src = ordered_hashes.get(i).unwrap();
            let dst = ordered_hashes.get(j).unwrap();
            let result_src_to_dst = index
                .query_reachability(&ctx, &repo.get_changeset_fetcher(), *src, *dst)
                .await;
            assert!(result_src_to_dst.unwrap());
            let result_dst_to_src = index
                .query_reachability(&ctx, &repo.get_changeset_fetcher(), *dst, *src)
                .await;
            assert_eq!(result_dst_to_src.unwrap(), src == dst);
        }
    }
}

pub async fn test_merge_uneven_reachability<T: ReachabilityIndex + 'static>(
    fb: FacebookInit,
    index_creator: fn() -> T,
) {
    let ctx = CoreContext::test_mock(fb);
    let repo = Arc::new(MergeUneven::getrepo(fb).await);
    let index = index_creator();
    let root_node = string_to_bonsai(&ctx, &repo, "15c40d0abc36d47fb51c8eaec51ac7aad31f669c").await;

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

    let _merge_node =
        string_to_bonsai(&ctx, &repo, "d35b1875cdd1ed2c687e86f1604b9d7e989450cb").await;

    for left_node in branch_1.into_iter() {
        for right_node in branch_2.iter() {
            assert!(
                index
                    .query_reachability(&ctx, &repo.get_changeset_fetcher(), left_node, root_node)
                    .await
                    .unwrap()
            );
            assert!(
                index
                    .query_reachability(&ctx, &repo.get_changeset_fetcher(), *right_node, root_node)
                    .await
                    .unwrap()
            );
            assert!(
                !index
                    .query_reachability(&ctx, &repo.get_changeset_fetcher(), root_node, left_node)
                    .await
                    .unwrap()
            );
            assert!(
                !index
                    .query_reachability(&ctx, &repo.get_changeset_fetcher(), root_node, *right_node)
                    .await
                    .unwrap()
            );
        }
    }
}

pub async fn test_branch_wide_reachability<T: ReachabilityIndex + 'static>(
    fb: FacebookInit,
    index_creator: fn() -> T,
) {
    let ctx = CoreContext::test_mock(fb);
    // this repo has no merges but many branches
    let repo = Arc::new(BranchWide::getrepo(fb).await);
    let index = index_creator();
    let root_node = string_to_bonsai(&ctx, &repo, "ecba698fee57eeeef88ac3dcc3b623ede4af47bd").await;

    let b1 = string_to_bonsai(&ctx, &repo, "9e8521affb7f9d10e9551a99c526e69909042b20").await;
    let b2 = string_to_bonsai(&ctx, &repo, "4685e9e62e4885d477ead6964a7600c750e39b03").await;
    let b1_1 = string_to_bonsai(&ctx, &repo, "b6a8169454af58b4b72b3665f9aa0d25529755ff").await;
    let b1_2 = string_to_bonsai(&ctx, &repo, "c27ef5b7f15e9930e5b93b1f32cc2108a2aabe12").await;
    let b2_1 = string_to_bonsai(&ctx, &repo, "04decbb0d1a65789728250ddea2fe8d00248e01c").await;
    let b2_2 = string_to_bonsai(&ctx, &repo, "49f53ab171171b3180e125b918bd1cf0af7e5449").await;

    // all nodes can reach the root
    for above_root in vec![b1, b2, b1_1, b1_2, b2_1, b2_2].iter() {
        assert!(
            index
                .query_reachability(&ctx, &repo.get_changeset_fetcher(), *above_root, root_node)
                .await
                .unwrap()
        );
        assert!(
            !index
                .query_reachability(&ctx, &repo.get_changeset_fetcher(), root_node, *above_root)
                .await
                .unwrap()
        );
    }

    // nodes in different branches cant reach each other
    for b1_node in vec![b1, b1_1, b1_2].iter() {
        for b2_node in vec![b2, b2_1, b2_2].iter() {
            assert!(
                !index
                    .query_reachability(&ctx, &repo.get_changeset_fetcher(), *b1_node, *b2_node)
                    .await
                    .unwrap()
            );
            assert!(
                !index
                    .query_reachability(&ctx, &repo.get_changeset_fetcher(), *b2_node, *b1_node)
                    .await
                    .unwrap()
            );
        }
    }

    // branch nodes can reach their common root but not each other
    // - branch 1
    assert!(
        index
            .query_reachability(&ctx, &repo.get_changeset_fetcher(), b1_1, b1)
            .await
            .unwrap()
    );
    assert!(
        index
            .query_reachability(&ctx, &repo.get_changeset_fetcher(), b1_2, b1)
            .await
            .unwrap()
    );
    assert!(
        !index
            .query_reachability(&ctx, &repo.get_changeset_fetcher(), b1_1, b1_2)
            .await
            .unwrap()
    );
    assert!(
        !index
            .query_reachability(&ctx, &repo.get_changeset_fetcher(), b1_2, b1_1)
            .await
            .unwrap()
    );

    // - branch 2
    assert!(
        index
            .query_reachability(&ctx, &repo.get_changeset_fetcher(), b2_1, b2)
            .await
            .unwrap()
    );
    assert!(
        index
            .query_reachability(&ctx, &repo.get_changeset_fetcher(), b2_2, b2)
            .await
            .unwrap()
    );
    assert!(
        !index
            .query_reachability(&ctx, &repo.get_changeset_fetcher(), b2_1, b2_2)
            .await
            .unwrap()
    );
    assert!(
        !index
            .query_reachability(&ctx, &repo.get_changeset_fetcher(), b2_2, b2_1)
            .await
            .unwrap()
    );
}

#[cfg(test)]
mod test {
    use super::*;

    use std::sync::Arc;

    use context::CoreContext;
    use fbinit::FacebookInit;
    use fixtures::Linear;
    use fixtures::TestRepoFixture;
    use mononoke_types::Generation;

    #[fbinit::test]
    async fn test_helpers(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(Linear::getrepo(fb).await);
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

        for (i, node) in ordered_hashes_oldest_to_newest.into_iter().enumerate() {
            assert_eq!(
                (
                    node,
                    fetch_generation(&ctx, &repo.get_changeset_fetcher(), node)
                        .await
                        .unwrap()
                ),
                (node, Generation::new(i as u64 + 1))
            );
        }
    }
}
