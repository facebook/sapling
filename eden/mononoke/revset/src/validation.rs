/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::setcommon::add_generations_by_bonsai;
use crate::setcommon::BonsaiInputStream;
use crate::BonsaiNodeStream;
use anyhow::Error;
use changeset_fetcher::ArcChangesetFetcher;
use context::CoreContext;
use futures_ext::StreamExt;
use futures_old::stream::Stream;
use futures_old::Async;
use futures_old::Poll;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;
use std::collections::HashSet;

/// A wrapper around a NodeStream that asserts that the two revset invariants hold:
/// 1. The generation number never increases
/// 2. No hash is seen twice
/// This uses memory proportional to the number of hashes in the revset.
pub struct ValidateNodeStream {
    wrapped: BonsaiInputStream,
    last_generation: Option<Generation>,
    seen_hashes: HashSet<ChangesetId>,
}

impl ValidateNodeStream {
    pub fn new(
        ctx: CoreContext,
        wrapped: BonsaiNodeStream,
        changeset_fetcher: &ArcChangesetFetcher,
    ) -> ValidateNodeStream {
        ValidateNodeStream {
            wrapped: add_generations_by_bonsai(ctx, wrapped, changeset_fetcher.clone()).boxify(),
            last_generation: None,
            seen_hashes: HashSet::new(),
        }
    }
}

impl Stream for ValidateNodeStream {
    type Item = ChangesetId;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let next = self.wrapped.poll()?;

        let (hash, gen) = match next {
            Async::NotReady => return Ok(Async::NotReady),
            Async::Ready(None) => return Ok(Async::Ready(None)),
            Async::Ready(Some((hash, gen))) => (hash, gen),
        };

        assert!(self.seen_hashes.insert(hash), "Hash {} seen twice", hash);

        assert!(
            self.last_generation.is_none() || self.last_generation >= Some(gen),
            "Generation number increased unexpectedly"
        );

        self.last_generation = Some(gen);

        Ok(Async::Ready(Some(hash)))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::fixtures::Linear;
    use crate::fixtures::TestRepoFixture;
    use crate::setcommon::NotReadyEmptyStream;
    use crate::tests::TestChangesetFetcher;
    use fbinit::FacebookInit;
    use futures::compat::Stream01CompatExt;
    use futures::stream::StreamExt as _;
    use futures_ext::StreamExt;
    use revset_test_helper::assert_changesets_sequence;
    use revset_test_helper::single_changeset_id;
    use revset_test_helper::string_to_bonsai;
    use std::sync::Arc;

    #[fbinit::test]
    async fn validate_accepts_single_node(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let repo = Linear::getrepo(fb).await;
        let changeset_fetcher: ArcChangesetFetcher =
            Arc::new(TestChangesetFetcher::new(repo.clone()));
        let repo = Arc::new(repo);

        let head_csid =
            string_to_bonsai(fb, &repo, "a5ffa77602a066db7d5cfb9fb5823a0895717c5a").await;

        let nodestream = single_changeset_id(ctx.clone(), head_csid.clone(), &repo).boxify();

        let nodestream =
            ValidateNodeStream::new(ctx.clone(), nodestream, &changeset_fetcher).boxify();
        assert_changesets_sequence(ctx, &repo, vec![head_csid], nodestream).await;
    }

    #[fbinit::test]
    async fn slow_ready_validates(fb: FacebookInit) {
        // Tests that we handle an input staying at NotReady for a while without panicking
        let ctx = CoreContext::test_mock(fb);
        let repo = Linear::getrepo(fb).await;
        let changeset_fetcher: ArcChangesetFetcher = Arc::new(TestChangesetFetcher::new(repo));

        let mut nodestream = ValidateNodeStream::new(
            ctx,
            NotReadyEmptyStream::new(10).boxify(),
            &changeset_fetcher,
        )
        .compat();

        assert!(nodestream.next().await.is_none());
    }

    #[fbinit::test]
    #[should_panic]
    async fn repeat_hash_panics(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(Linear::getrepo(fb).await);

        let head_csid =
            string_to_bonsai(fb, &repo, "a5ffa77602a066db7d5cfb9fb5823a0895717c5a").await;
        let nodestream = single_changeset_id(ctx.clone(), head_csid.clone(), &repo)
            .chain(single_changeset_id(ctx.clone(), head_csid.clone(), &repo));

        let changeset_fetcher: ArcChangesetFetcher =
            Arc::new(TestChangesetFetcher::new((*repo).clone()));

        let mut nodestream =
            ValidateNodeStream::new(ctx, nodestream.boxify(), &changeset_fetcher).boxify();

        loop {
            match nodestream.poll() {
                Ok(Async::Ready(None)) => return,
                _ => {}
            }
        }
    }

    #[fbinit::test]
    #[should_panic]
    async fn wrong_order_panics(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(Linear::getrepo(fb).await);

        let nodestream = single_changeset_id(
            ctx.clone(),
            string_to_bonsai(fb, &repo, "cb15ca4a43a59acff5388cea9648c162afde8372")
                .await
                .clone(),
            &repo,
        )
        .chain(single_changeset_id(
            ctx.clone(),
            string_to_bonsai(fb, &repo, "3c15267ebf11807f3d772eb891272b911ec68759").await,
            &repo,
        ));

        let changeset_fetcher: ArcChangesetFetcher =
            Arc::new(TestChangesetFetcher::new((*repo).clone()));

        let mut nodestream =
            ValidateNodeStream::new(ctx, nodestream.boxify(), &changeset_fetcher).boxify();

        loop {
            match nodestream.poll() {
                Ok(Async::Ready(None)) => return,
                _ => {}
            }
        }
    }
}
