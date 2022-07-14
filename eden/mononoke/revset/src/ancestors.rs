/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// The ancestors of the current node are itself, plus the union of all ancestors of all parents.
// Have a Vec of current generation nodes - as they're output, push their parents onto the next
// generation Vec. Once current generation Vec is empty, rotate.

use std::collections::hash_set::IntoIter;
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Error;
use cloned::cloned;
use futures::FutureExt;
use futures::TryFutureExt;
use futures_ext::StreamExt;
use futures_old::future::Future;
use futures_old::stream::iter_ok;
use futures_old::stream::Stream;
use futures_old::Async;
use futures_old::Poll;
use maplit::hashset;

use crate::UniqueHeap;
use changeset_fetcher::ArcChangesetFetcher;
use changeset_fetcher::ChangesetFetcher;
use context::CoreContext;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;

use crate::errors::*;
use crate::BonsaiNodeStream;
use crate::IntersectNodeStream;

pub struct AncestorsNodeStream {
    ctx: CoreContext,
    changeset_fetcher: ArcChangesetFetcher,
    next_generation: BTreeMap<Generation, HashSet<ChangesetId>>,
    pending_changesets: Box<dyn Stream<Item = (ChangesetId, Generation), Error = Error> + Send>,
    drain: IntoIter<ChangesetId>,

    // max heap of all relevant unique generation numbers
    sorted_unique_generations: UniqueHeap<Generation>,
}

fn make_pending(
    ctx: CoreContext,
    changeset_fetcher: ArcChangesetFetcher,
    hashes: IntoIter<ChangesetId>,
) -> Box<dyn Stream<Item = (ChangesetId, Generation), Error = Error> + Send> {
    let size = hashes.size_hint().0;

    Box::new(
        iter_ok::<_, Error>(hashes)
            .map({
                cloned!(ctx, changeset_fetcher);
                move |hash| {
                    cloned!(ctx, changeset_fetcher);
                    async move { changeset_fetcher.get_parents(ctx, hash).await }
                        .boxed()
                        .compat()
                        .map(|parents| parents.into_iter())
                        .map_err(|err| err.context(ErrorKind::ParentsFetchFailed))
                }
            })
            .buffered(size)
            .map(iter_ok::<_, Error>)
            .flatten()
            .and_then(move |node_cs| {
                cloned!(ctx, changeset_fetcher);
                async move {
                    changeset_fetcher
                        .get_generation_number(ctx.clone(), node_cs)
                        .await
                }
                .boxed()
                .compat()
                .map(move |gen_id| (node_cs, gen_id))
                .map_err(|err| err.context(ErrorKind::GenerationFetchFailed))
            }),
    )
}

impl AncestorsNodeStream {
    pub fn new(
        ctx: CoreContext,
        changeset_fetcher: &ArcChangesetFetcher,
        hash: ChangesetId,
    ) -> Self {
        let node_set: HashSet<ChangesetId> = hashset! {hash};
        AncestorsNodeStream {
            ctx: ctx.clone(),
            changeset_fetcher: changeset_fetcher.clone(),
            next_generation: BTreeMap::new(),
            pending_changesets: make_pending(
                ctx,
                changeset_fetcher.clone(),
                node_set.clone().into_iter(),
            ),
            drain: node_set.into_iter(),
            sorted_unique_generations: UniqueHeap::new(),
        }
    }
}

impl Stream for AncestorsNodeStream {
    type Item = ChangesetId;
    type Error = Error;
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // Empty the drain if any - return all items for this generation
        let next_in_drain = self.drain.next();
        if next_in_drain.is_some() {
            return Ok(Async::Ready(next_in_drain));
        }

        // Wait until we've drained pending_changesets - we can't continue until we know about all
        // parents of the just-output generation
        loop {
            match self.pending_changesets.poll()? {
                Async::Ready(Some((hash, generation))) => {
                    self.next_generation
                        .entry(generation)
                        .or_insert_with(HashSet::new)
                        .insert(hash);
                    // insert into our sorted list of generations
                    self.sorted_unique_generations.push(generation);
                }
                Async::NotReady => return Ok(Async::NotReady),
                Async::Ready(None) => break,
            };
        }

        if self.next_generation.is_empty() {
            // All parents output - nothing more to send
            return Ok(Async::Ready(None));
        }

        let highest_generation = self
            .sorted_unique_generations
            .pop()
            .expect("Expected a non empty heap of generations");
        let current_generation = self
            .next_generation
            .remove(&highest_generation)
            .expect("Highest generation doesn't exist");
        self.pending_changesets = make_pending(
            self.ctx.clone(),
            self.changeset_fetcher.clone(),
            current_generation.clone().into_iter(),
        );
        self.drain = current_generation.into_iter();
        Ok(Async::Ready(Some(self.drain.next().expect(
            "Cannot create a generation without at least one node hash",
        ))))
    }
}

pub fn common_ancestors<I>(
    ctx: CoreContext,
    changeset_fetcher: ArcChangesetFetcher,
    nodes: I,
) -> BonsaiNodeStream
where
    I: IntoIterator<Item = ChangesetId>,
{
    let nodes_iter = nodes.into_iter().map({
        cloned!(ctx, changeset_fetcher);
        move |node| AncestorsNodeStream::new(ctx.clone(), &changeset_fetcher, node).boxify()
    });

    IntersectNodeStream::new(ctx, &Arc::new(changeset_fetcher), nodes_iter).boxify()
}

pub fn greatest_common_ancestor<I>(
    ctx: CoreContext,
    changeset_fetcher: ArcChangesetFetcher,
    nodes: I,
) -> BonsaiNodeStream
where
    I: IntoIterator<Item = ChangesetId>,
{
    common_ancestors(ctx, changeset_fetcher, nodes)
        .take(1)
        .boxify()
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::fixtures::Linear;
    use crate::fixtures::MergeUneven;
    use crate::fixtures::TestRepoFixture;
    use crate::fixtures::UnsharedMergeUneven;
    use crate::tests::TestChangesetFetcher;
    use fbinit::FacebookInit;
    use revset_test_helper::assert_changesets_sequence;
    use revset_test_helper::string_to_bonsai;

    #[fbinit::test]
    async fn linear_ancestors(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let repo = Linear::getrepo(fb).await;
        let changeset_fetcher: ArcChangesetFetcher =
            Arc::new(TestChangesetFetcher::new(repo.clone()));
        let repo = Arc::new(repo);

        let nodestream = AncestorsNodeStream::new(
            ctx.clone(),
            &changeset_fetcher,
            string_to_bonsai(fb, &repo, "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157").await,
        )
        .boxify();

        assert_changesets_sequence(
            ctx.clone(),
            &repo,
            vec![
                string_to_bonsai(fb, &repo, "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157").await,
                string_to_bonsai(fb, &repo, "0ed509bf086fadcb8a8a5384dc3b550729b0fc17").await,
                string_to_bonsai(fb, &repo, "eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b").await,
                string_to_bonsai(fb, &repo, "cb15ca4a43a59acff5388cea9648c162afde8372").await,
                string_to_bonsai(fb, &repo, "d0a361e9022d226ae52f689667bd7d212a19cfe0").await,
                string_to_bonsai(fb, &repo, "607314ef579bd2407752361ba1b0c1729d08b281").await,
                string_to_bonsai(fb, &repo, "3e0e761030db6e479a7fb58b12881883f9f8c63f").await,
                string_to_bonsai(fb, &repo, "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").await,
            ],
            nodestream,
        )
        .await;
    }

    #[fbinit::test]
    async fn merge_ancestors_from_merge(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let repo = MergeUneven::getrepo(fb).await;
        let changeset_fetcher: ArcChangesetFetcher =
            Arc::new(TestChangesetFetcher::new(repo.clone()));
        let repo = Arc::new(repo);

        let nodestream = AncestorsNodeStream::new(
            ctx.clone(),
            &changeset_fetcher,
            string_to_bonsai(fb, &repo, "d35b1875cdd1ed2c687e86f1604b9d7e989450cb").await,
        )
        .boxify();

        assert_changesets_sequence(
            ctx.clone(),
            &repo,
            vec![
                string_to_bonsai(fb, &repo, "d35b1875cdd1ed2c687e86f1604b9d7e989450cb").await,
                string_to_bonsai(fb, &repo, "264f01429683b3dd8042cb3979e8bf37007118bc").await,
                string_to_bonsai(fb, &repo, "5d43888a3c972fe68c224f93d41b30e9f888df7c").await,
                string_to_bonsai(fb, &repo, "fc2cef43395ff3a7b28159007f63d6529d2f41ca").await,
                string_to_bonsai(fb, &repo, "bc7b4d0f858c19e2474b03e442b8495fd7aeef33").await,
                string_to_bonsai(fb, &repo, "795b8133cf375f6d68d27c6c23db24cd5d0cd00f").await,
                string_to_bonsai(fb, &repo, "4f7f3fd428bec1a48f9314414b063c706d9c1aed").await,
                string_to_bonsai(fb, &repo, "16839021e338500b3cf7c9b871c8a07351697d68").await,
                string_to_bonsai(fb, &repo, "1d8a907f7b4bf50c6a09c16361e2205047ecc5e5").await,
                string_to_bonsai(fb, &repo, "b65231269f651cfe784fd1d97ef02a049a37b8a0").await,
                string_to_bonsai(fb, &repo, "d7542c9db7f4c77dab4b315edd328edf1514952f").await,
                string_to_bonsai(fb, &repo, "3cda5c78aa35f0f5b09780d971197b51cad4613a").await,
                string_to_bonsai(fb, &repo, "15c40d0abc36d47fb51c8eaec51ac7aad31f669c").await,
            ],
            nodestream,
        )
        .await;
    }

    #[fbinit::test]
    async fn merge_ancestors_one_branch(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let repo = MergeUneven::getrepo(fb).await;
        let changeset_fetcher: ArcChangesetFetcher =
            Arc::new(TestChangesetFetcher::new(repo.clone()));
        let repo = Arc::new(repo);

        let nodestream = AncestorsNodeStream::new(
            ctx.clone(),
            &changeset_fetcher,
            string_to_bonsai(fb, &repo, "16839021e338500b3cf7c9b871c8a07351697d68").await,
        )
        .boxify();

        assert_changesets_sequence(
            ctx.clone(),
            &repo,
            vec![
                string_to_bonsai(fb, &repo, "16839021e338500b3cf7c9b871c8a07351697d68").await,
                string_to_bonsai(fb, &repo, "1d8a907f7b4bf50c6a09c16361e2205047ecc5e5").await,
                string_to_bonsai(fb, &repo, "3cda5c78aa35f0f5b09780d971197b51cad4613a").await,
                string_to_bonsai(fb, &repo, "15c40d0abc36d47fb51c8eaec51ac7aad31f669c").await,
            ],
            nodestream,
        )
        .await;
    }

    #[fbinit::test]
    async fn unshared_merge_all(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        // The UnsharedMergeUneven fixture has a commit after the merge. Pull in everything
        // by starting at the head and working back to the original unshared history commits
        let repo = UnsharedMergeUneven::getrepo(fb).await;
        let changeset_fetcher: ArcChangesetFetcher =
            Arc::new(TestChangesetFetcher::new(repo.clone()));
        let repo = Arc::new(repo);

        let nodestream = AncestorsNodeStream::new(
            ctx.clone(),
            &changeset_fetcher,
            string_to_bonsai(fb, &repo, "dd993aab2bed7276e17c88470286ba8459ba6d94").await,
        )
        .boxify();

        assert_changesets_sequence(
            ctx.clone(),
            &repo,
            vec![
                string_to_bonsai(fb, &repo, "dd993aab2bed7276e17c88470286ba8459ba6d94").await,
                string_to_bonsai(fb, &repo, "9c6dd4e2c2f43c89613b094efb426cc42afdee2a").await,
                string_to_bonsai(fb, &repo, "64011f64aaf9c2ad2e674f57c033987da4016f51").await,
                string_to_bonsai(fb, &repo, "c1d5375bf73caab8725d759eaca56037c725c7d1").await,
                string_to_bonsai(fb, &repo, "e819f2dd9a01d3e63d9a93e298968df275e6ad7c").await,
                string_to_bonsai(fb, &repo, "5a3e8d5a475ec07895e64ec1e1b2ec09bfa70e4e").await,
                string_to_bonsai(fb, &repo, "76096af83f52cc9a225ccfd8ddfb05ea18132343").await,
                string_to_bonsai(fb, &repo, "33fb49d8a47b29290f5163e30b294339c89505a2").await,
                string_to_bonsai(fb, &repo, "03b0589d9788870817d03ce7b87516648ed5b33a").await,
                string_to_bonsai(fb, &repo, "2fa8b4ee6803a18db4649a3843a723ef1dfe852b").await,
                string_to_bonsai(fb, &repo, "f01e186c165a2fbe931fd1bf4454235398c591c9").await,
                string_to_bonsai(fb, &repo, "163adc0d0f5d2eb0695ca123addcb92bab202096").await,
                string_to_bonsai(fb, &repo, "0b94a2881dda90f0d64db5fae3ee5695a38e7c8f").await,
                string_to_bonsai(fb, &repo, "eee492dcdeaae18f91822c4359dd516992e0dbcd").await,
                string_to_bonsai(fb, &repo, "f61fdc0ddafd63503dcd8eed8994ec685bfc8941").await,
                string_to_bonsai(fb, &repo, "3775a86c64cceeaf68ffe3f012fc90774c42002b").await,
                string_to_bonsai(fb, &repo, "36ff88dd69c9966c9fad9d6d0457c52153039dde").await,
                string_to_bonsai(fb, &repo, "1700524113b1a3b1806560341009684b4378660b").await,
                string_to_bonsai(fb, &repo, "9d374b7e8180f933e3043ad1ffab0a9f95e2bac6").await,
            ],
            nodestream,
        )
        .await;
    }

    #[fbinit::test]
    async fn no_common_ancestor(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let repo = UnsharedMergeUneven::getrepo(fb).await;
        let changeset_fetcher: ArcChangesetFetcher =
            Arc::new(TestChangesetFetcher::new(repo.clone()));
        let repo = Arc::new(repo);

        let nodestream = greatest_common_ancestor(
            ctx.clone(),
            changeset_fetcher,
            vec![
                string_to_bonsai(fb, &repo, "64011f64aaf9c2ad2e674f57c033987da4016f51").await,
                string_to_bonsai(fb, &repo, "1700524113b1a3b1806560341009684b4378660b").await,
            ],
        );
        assert_changesets_sequence(ctx.clone(), &repo, vec![], nodestream).await;
    }

    #[fbinit::test]
    async fn greatest_common_ancestor_different_branches(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let repo = MergeUneven::getrepo(fb).await;
        let changeset_fetcher: ArcChangesetFetcher =
            Arc::new(TestChangesetFetcher::new(repo.clone()));
        let repo = Arc::new(repo);

        let nodestream = greatest_common_ancestor(
            ctx.clone(),
            changeset_fetcher,
            vec![
                string_to_bonsai(fb, &repo, "4f7f3fd428bec1a48f9314414b063c706d9c1aed").await,
                string_to_bonsai(fb, &repo, "3cda5c78aa35f0f5b09780d971197b51cad4613a").await,
            ],
        );
        assert_changesets_sequence(
            ctx.clone(),
            &repo,
            vec![string_to_bonsai(fb, &repo, "15c40d0abc36d47fb51c8eaec51ac7aad31f669c").await],
            nodestream,
        )
        .await;
    }

    #[fbinit::test]
    async fn greatest_common_ancestor_same_branch(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let repo = MergeUneven::getrepo(fb).await;
        let changeset_fetcher: ArcChangesetFetcher =
            Arc::new(TestChangesetFetcher::new(repo.clone()));
        let repo = Arc::new(repo);

        let nodestream = greatest_common_ancestor(
            ctx.clone(),
            changeset_fetcher,
            vec![
                string_to_bonsai(fb, &repo, "4f7f3fd428bec1a48f9314414b063c706d9c1aed").await,
                string_to_bonsai(fb, &repo, "264f01429683b3dd8042cb3979e8bf37007118bc").await,
            ],
        );
        assert_changesets_sequence(
            ctx.clone(),
            &repo,
            vec![string_to_bonsai(fb, &repo, "4f7f3fd428bec1a48f9314414b063c706d9c1aed").await],
            nodestream,
        )
        .await;
    }

    #[fbinit::test]
    async fn all_common_ancestors_different_branches(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let repo = MergeUneven::getrepo(fb).await;
        let changeset_fetcher: ArcChangesetFetcher =
            Arc::new(TestChangesetFetcher::new(repo.clone()));
        let repo = Arc::new(repo);

        let nodestream = common_ancestors(
            ctx.clone(),
            changeset_fetcher,
            vec![
                string_to_bonsai(fb, &repo, "4f7f3fd428bec1a48f9314414b063c706d9c1aed").await,
                string_to_bonsai(fb, &repo, "3cda5c78aa35f0f5b09780d971197b51cad4613a").await,
            ],
        );
        assert_changesets_sequence(
            ctx.clone(),
            &repo,
            vec![string_to_bonsai(fb, &repo, "15c40d0abc36d47fb51c8eaec51ac7aad31f669c").await],
            nodestream,
        )
        .await;
    }

    #[fbinit::test]
    async fn all_common_ancestors_same_branch(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let repo = MergeUneven::getrepo(fb).await;
        let changeset_fetcher: ArcChangesetFetcher =
            Arc::new(TestChangesetFetcher::new(repo.clone()));
        let repo = Arc::new(repo);

        let nodestream = common_ancestors(
            ctx.clone(),
            changeset_fetcher,
            vec![
                string_to_bonsai(fb, &repo, "4f7f3fd428bec1a48f9314414b063c706d9c1aed").await,
                string_to_bonsai(fb, &repo, "264f01429683b3dd8042cb3979e8bf37007118bc").await,
            ],
        );
        assert_changesets_sequence(
            ctx.clone(),
            &repo,
            vec![
                string_to_bonsai(fb, &repo, "4f7f3fd428bec1a48f9314414b063c706d9c1aed").await,
                string_to_bonsai(fb, &repo, "b65231269f651cfe784fd1d97ef02a049a37b8a0").await,
                string_to_bonsai(fb, &repo, "d7542c9db7f4c77dab4b315edd328edf1514952f").await,
                string_to_bonsai(fb, &repo, "15c40d0abc36d47fb51c8eaec51ac7aad31f669c").await,
            ],
            nodestream,
        )
        .await;
    }
}
